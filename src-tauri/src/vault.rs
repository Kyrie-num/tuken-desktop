//! Signed-in accounts, the pending login and shop orders. Kept in one
//! encrypted file: with DPAPI for this Windows user, and on macOS with a
//! random key that lives in the login keychain. Desktop tokens and the shop's
//! order-lookup passwords never reach web storage or logs.

use std::io::Write;
use std::path::Path;

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

#[cfg(target_os = "macos")]
fn protect(data: &[u8], decrypt: bool, app: &str) -> Result<Vec<u8>, String> {
    if data.len() > 8 * 1024 * 1024 {
        return Err("账户文件过大".into());
    }
    match keychain::key(app, !decrypt)? {
        Some(key) if decrypt => sealed::open(&key, data),
        Some(key) => sealed::seal(&key, data),
        // The key is gone (the keychain was reset), and what it locked with
        // it can't be read again: start over, signed out. The next save
        // makes a new key.
        None => {
            log::warn!("the vault's keychain item is missing; starting over");
            Ok(b"{}".to_vec())
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn protect(_: &[u8], _: bool, _: &str) -> Result<Vec<u8>, String> {
    Err("兔啃目前只支持 Windows 和 macOS".into())
}

/// The vault's key on macOS: 32 random bytes, a generic password in the login
/// keychain (service: the app's identifier). Only 兔啃 itself may read it
/// without asking; macOS asks the user before any other app gets it.
#[cfg(target_os = "macos")]
mod keychain {
    use std::sync::Mutex;

    use security_framework::passwords::{generic_password, set_generic_password, PasswordOptions};

    const ACCOUNT: &str = "vault";
    /// errSecItemNotFound.
    const NOT_FOUND: i32 = -25300;

    /// Read once per run: macOS may ask the user, and should only once.
    static KEYS: Mutex<Vec<(String, [u8; 32])>> = Mutex::new(Vec::new());

    /// The key, made and stored on first use when `create`; `None` when there
    /// is none and `create` is false.
    pub fn key(service: &str, create: bool) -> Result<Option<[u8; 32]>, String> {
        let mut keys = KEYS.lock().unwrap();
        if let Some((_, key)) = keys.iter().find(|(s, _)| s == service) {
            return Ok(Some(*key));
        }
        let key = match generic_password(PasswordOptions::new_generic_password(service, ACCOUNT)) {
            Ok(bytes) => <[u8; 32]>::try_from(bytes.as_slice())
                .map_err(|_| "钥匙串里兔啃的密钥损坏了，请在「钥匙串访问」里删掉它再重开兔啃")?,
            Err(e) if e.code() == NOT_FOUND => {
                if !create {
                    return Ok(None);
                }
                let key = super::sealed::new_key()?;
                set_generic_password(service, ACCOUNT, &key)
                    .map_err(|_| "无法把密钥存进钥匙串，请确认钥匙串已解锁")?;
                key
            }
            Err(e) => {
                log::warn!("keychain read failed: {}", e.code());
                return Err(
                    "钥匙串没有允许兔啃读取登录信息，请重开兔啃，在弹窗里选「始终允许」".into(),
                );
            }
        };
        keys.push((service.to_string(), key));
        Ok(Some(key))
    }

    /// Removes a key; for tests, which use their own service names.
    #[cfg(test)]
    pub fn forget(service: &str) {
        let _ = security_framework::passwords::delete_generic_password(service, ACCOUNT);
        KEYS.lock().unwrap().retain(|(s, _)| s != service);
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

/// `app`: the app's identifier (see `protect`).
pub fn load(path: &Path, app: &str) -> Result<Vault, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&protect(&bytes, true, app)?)
            .map_err(|_| "账户文件损坏，请重新登录".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vault::default()),
        Err(_) => Err("无法读取账户文件".into()),
    }
}

pub fn save(path: &Path, app: &str, v: &Vault) -> Result<(), String> {
    let dir = path.parent().ok_or("账户目录无效")?;
    std::fs::create_dir_all(dir).map_err(|_| "无法创建账户目录")?;
    let plain = serde_json::to_vec(v).map_err(|_| "无法保存账户")?;
    let data = protect(&plain, false, app)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法保存账户")?;
    tmp.write_all(&data).map_err(|_| "无法保存账户")?;
    tmp.as_file().sync_all().map_err(|_| "无法保存账户")?;
    tmp.persist(path).map_err(|_| "无法保存账户文件")?;
    Ok(())
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
        let path = dir.path().join("vault");
        let mut v = Vault::default();
        v.upsert(account(7));
        v.active = Some(7);
        save(&path, &app, &v).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("tld_secret_7"));
        assert_eq!(load(&path, &app).unwrap().current().unwrap().user_id, 7);
        #[cfg(target_os = "macos")]
        {
            // Without its key the file can't be read: 兔啃 starts over, signed out.
            keychain::forget(&app);
            assert!(load(&path, &app).unwrap().current().is_none());
        }
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
