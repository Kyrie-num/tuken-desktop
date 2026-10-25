//! Signed-in accounts, the pending login and shop orders. Kept in one
//! encrypted file: with DPAPI for this Windows user, and on macOS with a
//! random key that lives in the login keychain. Desktop tokens and the shop's
//! order-lookup passwords never reach web storage or logs.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub token: String,
    pub user_id: i64,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Login {
    pub device: String,
    pub verifier: String,
    pub code: String,
    pub expires: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub owner: i64,
    pub goods: String,
    pub name: String,
    pub amount: f64,
    pub contact: String,
    pub password: String,
    pub trade: Option<String>,
    pub pay_url: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub message: String,
}

/// Orders the worker still has to finish.
pub const OPEN: [&str; 3] = ["awaiting_payment", "awaiting_delivery", "redeeming"];

impl Order {
    pub fn is_open(&self) -> bool {
        OPEN.contains(&self.status.as_str())
    }
    /// What the UI may see: no contact, password or payment link.
    pub fn public(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "amount": self.amount,
            "trade_no": self.trade,
            "status": self.status,
            "created_at": self.created_at,
            "message": self.message,
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub active: Option<i64>,
    #[serde(default)]
    pub login: Option<Login>,
    #[serde(default)]
    pub orders: Vec<Order>,
}

impl Vault {
    pub fn account(&self, user_id: i64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.user_id == user_id)
    }
    /// The account the UI works with.
    pub fn current(&self) -> Option<&Account> {
        self.active.and_then(|id| self.account(id))
    }
    /// Adds the account, or refreshes its token and names if it is known.
    pub fn upsert(&mut self, account: Account) {
        match self
            .accounts
            .iter_mut()
            .find(|a| a.user_id == account.user_id)
        {
            Some(a) => *a = account,
            None => self.accounts.push(account),
        }
    }
    /// Forgets an account; the next one, if any, becomes current.
    pub fn remove(&mut self, user_id: i64) {
        self.accounts.retain(|a| a.user_id != user_id);
        if self.active == Some(user_id) {
            self.active = self.accounts.first().map(|a| a.user_id);
        }
    }
    /// The state the UI renders: accounts without tokens, and the current
    /// account's orders.
    pub fn view(&self) -> Value {
        json!({
            "active": self.current().map(|a| a.user_id),
            "accounts": self.accounts.iter().map(|a| json!({
                "user_id": a.user_id,
                "email": a.email,
                "username": a.username,
            })).collect::<Vec<_>>(),
            "orders": self.orders.iter()
                .filter(|o| Some(o.owner) == self.active)
                .map(Order::public)
                .collect::<Vec<_>>(),
        })
    }
}

/// `app`: the app's identifier, which names the macOS keychain item, so a dev
/// build (`site.tuken.desktop.dev`) keeps its own. DPAPI needs no name.
#[cfg(windows)]
fn protect(data: &[u8], decrypt: bool, app: &str) -> Result<Vec<u8>, String> {
    let _ = app;
    #[repr(C)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            label: *const u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            label: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    // CRYPTPROTECT_UI_FORBIDDEN: never show a prompt.
    const NO_UI: u32 = 1;
    if data.len() > 8 * 1024 * 1024 {
        return Err("账户文件过大".into());
    }
    let input = Blob {
        size: data.len() as u32,
        data: data.as_ptr() as *mut u8,
    };
    let mut out = Blob {
        size: 0,
        data: std::ptr::null_mut(),
    };
    unsafe {
        let ok = if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                NO_UI,
                &mut out,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                NO_UI,
                &mut out,
            )
        };
        if ok == 0 {
            return Err("Windows 无法读取账户文件，请用原来的 Windows 用户打开兔啃".into());
        }
        let bytes = std::slice::from_raw_parts(out.data, out.size as usize).to_vec();
        LocalFree(out.data.cast());
        Ok(bytes)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn protect(_: &[u8], _: bool, _: &str) -> Result<Vec<u8>, String> {
    Err("兔啃目前只支持 Windows 和 macOS".into())
}

/// What the keychain has for the vault: its key, no key at all, or nothing
/// 兔啃 may use now (the keychain said no, or the item isn't a key).
#[cfg(any(target_os = "macos", test))]
enum KeyLookup {
    Found([u8; 32]),
    Missing,
    Unusable(String),
}

/// macOS: the vault's key is 32 random bytes, a generic password in the
/// login keychain named after the app's identifier (`site.tuken.desktop` in
/// 钥匙串访问; a dev build has its own `.dev` one).
///
/// Only 兔啃 reads it without asking. After an update macOS asks once: it
/// files each keychain item under the code that made it (its "partition",
/// for ad-hoc signed code the code's hash), so a new build of 兔啃 is another
/// app to it until the user lets it in ("始终允许"). An access list for any
/// application doesn't change that (tested on GitHub's Macs: the item still
/// gets `partition_id: cdhash:…`, and moving it takes the keychain password).
/// With a Developer ID the partition becomes the team and updates read it
/// unasked.
///
/// The key is only ever added, never replaced: whatever it sealed would be
/// lost with it.
#[cfg(target_os = "macos")]
mod keychain {
    use std::ptr;
    use std::sync::Mutex;

    use core_foundation::base::TCFType;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use core_foundation_sys::base::OSStatus;
    use core_foundation_sys::string::CFStringRef;
    use security_framework::passwords::{generic_password, PasswordOptions};
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecValueData,
    };
    use security_framework_sys::keychain_item::SecItemAdd;

    use super::KeyLookup;

    pub const ACCOUNT: &str = "vault";
    /// errSecItemNotFound, errSecDuplicateItem.
    const NOT_FOUND: OSStatus = -25300;
    const DUPLICATE: OSStatus = -25299;

    /// Read once per run: macOS may ask the user, and should only once.
    static KEYS: Mutex<Vec<(String, [u8; 32])>> = Mutex::new(Vec::new());

    fn remember(service: &str, key: [u8; 32]) -> [u8; 32] {
        KEYS.lock().unwrap().push((service.to_string(), key));
        key
    }

    /// The key, if the keychain has one for `service`.
    pub fn lookup(service: &str) -> KeyLookup {
        if let Some((_, key)) = KEYS.lock().unwrap().iter().find(|(s, _)| s == service) {
            return KeyLookup::Found(*key);
        }
        match generic_password(PasswordOptions::new_generic_password(service, ACCOUNT)) {
            Ok(bytes) => match <[u8; 32]>::try_from(bytes.as_slice()) {
                Ok(key) => KeyLookup::Found(remember(service, key)),
                Err(_) => KeyLookup::Unusable(format!(
                    "钥匙串里的「{service}」不是兔啃的密钥。请在「钥匙串访问」里删掉这一项，再点重试"
                )),
            },
            Err(e) if e.code() == NOT_FOUND => KeyLookup::Missing,
            Err(e) => {
                log::warn!("keychain read of {service} failed: {}", e.code());
                KeyLookup::Unusable(format!(
                    "钥匙串没有让兔啃读取「{service}」，登录信息暂时打不开。点重试，在钥匙串的弹窗里输入电脑密码，选「始终允许」"
                ))
            }
        }
    }

    /// The key for `service`, made and added on first use.
    pub fn key(service: &str) -> Result<[u8; 32], String> {
        let failed = || format!("没法把密钥存进钥匙串「{service}」，请确认钥匙串已解锁后重试");
        match lookup(service) {
            KeyLookup::Found(key) => Ok(key),
            KeyLookup::Unusable(why) => Err(why),
            KeyLookup::Missing => {
                let key = super::sealed::new_key()?;
                match unsafe { add(service, &key) } {
                    0 => Ok(remember(service, key)),
                    // Someone added one meanwhile: that one is the key.
                    DUPLICATE => match lookup(service) {
                        KeyLookup::Found(key) => Ok(key),
                        KeyLookup::Unusable(why) => Err(why),
                        KeyLookup::Missing => Err(failed()),
                    },
                    status => {
                        log::warn!("keychain add of {service} failed: {status}");
                        Err(failed())
                    }
                }
            }
        }
    }

    /// Adds the key. Fails with `errSecDuplicateItem` rather than replace a
    /// key that is there (SecItemAdd alone, no update).
    unsafe fn add(service: &str, key: &[u8; 32]) -> OSStatus {
        let name = |s: CFStringRef| CFString::wrap_under_get_rule(s);
        let query = CFDictionary::from_CFType_pairs(&[
            (name(kSecClass), name(kSecClassGenericPassword).as_CFType()),
            (name(kSecAttrService), CFString::new(service).as_CFType()),
            (name(kSecAttrAccount), CFString::new(ACCOUNT).as_CFType()),
            (name(kSecValueData), CFData::from_buffer(key).as_CFType()),
        ]);
        SecItemAdd(query.as_concrete_TypeRef(), ptr::null_mut())
    }

    /// Forgets what this run read, as a new run would; for tests.
    #[cfg(test)]
    pub fn forget_read(service: &str) {
        KEYS.lock().unwrap().retain(|(s, _)| s != service);
    }

    /// Removes a key; for tests, which use their own service names.
    #[cfg(test)]
    pub fn forget(service: &str) {
        let _ = security_framework::passwords::delete_generic_password(service, ACCOUNT);
        forget_read(service);
    }
}

/// The macOS vault file: `TKV1`, a random 96-bit nonce, then the AES-256-GCM
/// sealed JSON (the magic is authenticated too). A fresh nonce per save; the
/// vault is saved far fewer than 2^32 times.
#[cfg(any(target_os = "macos", test))]
mod sealed {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
    use ring::rand::{SecureRandom, SystemRandom};

    const MAGIC: &[u8; 4] = b"TKV1";
    const BROKEN: &str = "账户文件损坏，请重新登录";

    pub fn new_key() -> Result<[u8; 32], String> {
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| "无法生成密钥")?;
        Ok(key)
    }

    fn cipher(key: &[u8; 32]) -> LessSafeKey {
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("a 32-byte AES-256 key"))
    }

    pub fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| "无法保存账户")?;
        let mut body = plain.to_vec();
        cipher(key)
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(MAGIC),
                &mut body,
            )
            .map_err(|_| "无法保存账户")?;
        Ok([&MAGIC[..], &nonce, &body].concat())
    }

    pub fn open(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
        let rest = data.strip_prefix(MAGIC).ok_or(BROKEN)?;
        if rest.len() < NONCE_LEN {
            return Err(BROKEN.into());
        }
        let (nonce, body) = rest.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| BROKEN)?;
        let mut body = body.to_vec();
        let plain = cipher(key)
            .open_in_place(nonce, Aad::from(MAGIC), &mut body)
            .map_err(|_| BROKEN)?;
        Ok(plain.to_vec())
    }
}

/// What loading the vault found.
pub struct Loaded {
    pub vault: Vault,
    /// macOS: the file, moved aside to this name, when it can't be read here
    /// any more (its key is gone, or the key there isn't its key). The user
    /// signs in again; the file itself is kept.
    pub lost: Option<PathBuf>,
}

/// `app`: the app's identifier (see `protect`).
pub fn load(path: &Path, app: &str) -> Result<Loaded, String> {
    match std::fs::read(path) {
        Ok(bytes) => open(path, &bytes, app),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Loaded {
            vault: Vault::default(),
            lost: None,
        }),
        Err(_) => Err("无法读取账户文件".into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn open(_: &Path, bytes: &[u8], app: &str) -> Result<Loaded, String> {
    serde_json::from_slice(&protect(bytes, true, app)?)
        .map(|vault| Loaded { vault, lost: None })
        .map_err(|_| "账户文件损坏，请重新登录".into())
}

#[cfg(target_os = "macos")]
fn open(path: &Path, bytes: &[u8], app: &str) -> Result<Loaded, String> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("账户文件过大".into());
    }
    open_sealed(path, bytes, keychain::lookup(app))
}

/// Opens a sealed vault with what the keychain gave. A refusal leaves the
/// file as it is, to be read once the keychain lets 兔啃. A file with no key
/// left, or one its key can't open, is moved aside, never overwritten.
#[cfg(any(target_os = "macos", test))]
fn open_sealed(path: &Path, bytes: &[u8], key: KeyLookup) -> Result<Loaded, String> {
    let key = match key {
        KeyLookup::Found(key) => Some(key),
        KeyLookup::Missing => None,
        KeyLookup::Unusable(why) => return Err(why),
    };
    let vault = key
        .and_then(|key| sealed::open(&key, bytes).ok())
        .and_then(|plain| serde_json::from_slice(&plain).ok());
    match vault {
        Some(vault) => Ok(Loaded { vault, lost: None }),
        None => {
            let lost = set_aside(path)?;
            log::warn!("the vault can't be opened; moved to {}", lost.display());
            Ok(Loaded {
                vault: Vault::default(),
                lost: Some(lost),
            })
        }
    }
}

/// Moves the file to `<name>.lost-<time>` beside it.
#[cfg(any(target_os = "macos", test))]
fn set_aside(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut lost = path.with_file_name(format!("{name}.lost-{stamp}"));
    let mut n = 1;
    while lost.exists() {
        n += 1;
        lost = path.with_file_name(format!("{name}.lost-{stamp}-{n}"));
    }
    std::fs::rename(path, &lost).map_err(|_| "登录信息读不出来，也没法把它移开，请重开兔啃再试")?;
    Ok(lost)
}

pub fn save(path: &Path, app: &str, v: &Vault) -> Result<(), String> {
    let dir = path.parent().ok_or("账户目录无效")?;
    std::fs::create_dir_all(dir).map_err(|_| "无法创建账户目录")?;
    let plain = serde_json::to_vec(v).map_err(|_| "无法保存账户")?;
    let data = seal(&plain, app)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法保存账户")?;
    tmp.write_all(&data).map_err(|_| "无法保存账户")?;
    tmp.as_file().sync_all().map_err(|_| "无法保存账户")?;
    tmp.persist(path).map_err(|_| "无法保存账户文件")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn seal(plain: &[u8], app: &str) -> Result<Vec<u8>, String> {
    protect(plain, false, app)
}

#[cfg(target_os = "macos")]
fn seal(plain: &[u8], app: &str) -> Result<Vec<u8>, String> {
    if plain.len() > 8 * 1024 * 1024 {
        return Err("账户文件过大".into());
    }
    sealed::seal(&keychain::key(app)?, plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: i64) -> Account {
        Account {
            token: format!("tld_secret_{id}"),
            user_id: id,
            email: format!("u{id}@example.test"),
            username: String::new(),
        }
    }

    #[test]
    fn switching_and_removing_accounts() {
        let mut v = Vault::default();
        v.upsert(account(1));
        v.upsert(account(2));
        v.active = Some(2);
        assert_eq!(v.current().unwrap().user_id, 2);
        // A second login of the same user refreshes, not duplicates.
        v.upsert(Account {
            token: "tld_new".into(),
            ..account(2)
        });
        assert_eq!(v.accounts.len(), 2);
        assert_eq!(v.current().unwrap().token, "tld_new");
        v.remove(2);
        assert_eq!(v.active, Some(1));
        v.remove(1);
        assert_eq!(v.active, None);
    }

    #[test]
    fn view_hides_tokens_and_other_accounts_orders() {
        let mut v = Vault::default();
        v.upsert(account(1));
        v.upsert(account(2));
        v.active = Some(1);
        v.orders.push(Order {
            id: "o".into(),
            owner: 2,
            goods: "g".into(),
            name: "1刀额度".into(),
            amount: 1.0,
            contact: "secret-contact".into(),
            password: "secret-password".into(),
            trade: None,
            pay_url: Some("https://pay.example/secret".into()),
            status: "awaiting_payment".into(),
            created_at: 0,
            message: String::new(),
        });
        let s = v.view().to_string();
        assert!(!s.contains("tld_secret"));
        assert!(!s.contains("secret-"));
        assert_eq!(v.view()["orders"].as_array().unwrap().len(), 0);
        assert_eq!(v.view()["accounts"].as_array().unwrap().len(), 2);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn vault_round_trip_is_encrypted() {
        // On macOS a keychain item of its own, removed again at the end.
        let app = format!("site.tuken.desktop.test.{}", uuid::Uuid::new_v4());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tuken.vault");
        let mut v = Vault::default();
        v.upsert(account(7));
        v.active = Some(7);
        save(&path, &app, &v).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("tld_secret_7"));
        let loaded = load(&path, &app).unwrap();
        assert_eq!(loaded.vault.current().unwrap().user_id, 7);
        assert!(loaded.lost.is_none());
        #[cfg(target_os = "macos")]
        {
            // Without its key the file can't be read any more: it is moved
            // aside, byte for byte, and 兔啃 starts signed out.
            keychain::forget(&app);
            let loaded = load(&path, &app).unwrap();
            assert!(loaded.vault.current().is_none());
            assert_eq!(std::fs::read(loaded.lost.unwrap()).unwrap(), raw);
            assert!(!path.exists());
            // The next save makes a new key and a new file beside it.
            save(&path, &app, &v).unwrap();
            assert_eq!(
                load(&path, &app).unwrap().vault.current().unwrap().user_id,
                7
            );
            keychain::forget(&app);
        }
    }

    #[test]
    fn an_unreadable_vault_is_set_aside_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tuken.vault");
        let key = sealed::new_key().unwrap();
        let mut v = Vault::default();
        v.upsert(account(3));
        v.active = Some(3);
        let bytes = sealed::seal(&key, &serde_json::to_vec(&v).unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();

        // With its key it opens, where it is.
        let loaded = open_sealed(&path, &bytes, KeyLookup::Found(key)).unwrap();
        assert_eq!(loaded.vault.current().unwrap().user_id, 3);
        assert!(loaded.lost.is_none() && path.exists());
        // The keychain says no: an error, and the file stays untouched.
        assert!(open_sealed(&path, &bytes, KeyLookup::Unusable("no".into())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        // No key, another key, or not a vault at all: moved aside under a new
        // name, byte for byte, and 兔啃 starts signed out.
        let other = sealed::new_key().unwrap();
        for (lookup, data) in [
            (KeyLookup::Missing, bytes.clone()),
            (KeyLookup::Found(other), bytes.clone()),
            (KeyLookup::Found(key), b"TKV1 not sealed".to_vec()),
        ] {
            std::fs::write(&path, &data).unwrap();
            let loaded = open_sealed(&path, &data, lookup).unwrap();
            assert!(loaded.vault.accounts.is_empty() && loaded.vault.active.is_none());
            let lost = loaded.lost.unwrap();
            let name = lost.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("tuken.vault.lost-"), "{name}");
            assert_eq!(std::fs::read(&lost).unwrap(), data);
            assert!(!path.exists());
        }
        // Each one kept, even within the same second.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    /// Keys are only ever added: one that is there is the key, and a new
    /// one stays the key for the next run.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_keychain_key_is_only_added_never_replaced() {
        let service = format!("site.tuken.desktop.test.{}", uuid::Uuid::new_v4());
        let theirs = [7u8; 32];
        security_framework::passwords::set_generic_password(&service, keychain::ACCOUNT, &theirs)
            .unwrap();
        assert_eq!(keychain::key(&service).unwrap(), theirs);
        keychain::forget(&service);

        let made = keychain::key(&service).unwrap();
        keychain::forget_read(&service);
        assert_eq!(keychain::key(&service).unwrap(), made);
        keychain::forget(&service);
    }

    /// Why 兔啃 asks for the keychain after an update while it is ad-hoc
    /// signed: a copy of this test program signed anew, as an update is,
    /// can't read the key without the user. If this ever fails, macOS
    /// stopped asking and README can say so. It adds a keychain item and
    /// removes it: `cargo test macos_keychain -- --ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn macos_keychain_asks_once_after_an_update() {
        use std::process::Command;
        use std::time::{Duration, Instant};
        let service = format!("site.tuken.desktop.test.{}", uuid::Uuid::new_v4());
        keychain::key(&service).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let update = dir.path().join("update");
        std::fs::copy(std::env::current_exe().unwrap(), &update).unwrap();
        let signed = Command::new("/usr/bin/codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--identifier",
                "site.tuken.update",
            ])
            .arg(&update)
            .status()
            .unwrap();
        assert!(signed.success());
        // Its own report would read as a failure in the log; only its exit counts.
        let mut child = Command::new(&update)
            .args(["--exact", "vault::tests::macos_keychain_read", "--ignored"])
            .env("TUKEN_KEYCHAIN_READ", &service)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let read = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status.success();
            }
            if started.elapsed() > Duration::from_secs(60) {
                let _ = child.kill();
                break false;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        keychain::forget(&service);
        assert!(
            !read,
            "a new build read the key unasked; README can drop its note"
        );
    }

    /// The new build's side of the test above.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn macos_keychain_read() {
        let Ok(service) = std::env::var("TUKEN_KEYCHAIN_READ") else {
            return;
        };
        // Fail instead of asking anyone.
        unsafe { security_framework_sys::keychain::SecKeychainSetUserInteractionAllowed(0) };
        let read = security_framework::passwords::generic_password(
            security_framework::passwords::PasswordOptions::new_generic_password(
                &service,
                keychain::ACCOUNT,
            ),
        );
        assert!(
            read.is_ok(),
            "keychain said {:?}",
            read.err().map(|e| e.code())
        );
    }

    #[test]
    fn sealed_files_need_their_key_and_stay_intact() {
        let key = sealed::new_key().unwrap();
        let plain = br#"{"accounts":[{"token":"tld_secret"}]}"#;
        let a = sealed::seal(&key, plain).unwrap();
        let b = sealed::seal(&key, plain).unwrap();
        assert!(a.starts_with(b"TKV1"));
        // A fresh nonce each time, and no plain text in the file.
        assert_ne!(a, b);
        assert!(!String::from_utf8_lossy(&a).contains("tld_secret"));
        assert_eq!(sealed::open(&key, &a).unwrap(), plain);
        // Another key, a flipped bit, a cut file or another format: refused.
        assert!(sealed::open(&sealed::new_key().unwrap(), &a).is_err());
        let mut flipped = a.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert!(sealed::open(&key, &flipped).is_err());
        assert!(sealed::open(&key, &a[..20]).is_err());
        assert!(sealed::open(&key, b"{}").is_err());
    }
}
