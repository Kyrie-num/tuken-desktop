//! Signed-in accounts, the pending login and shop orders. Kept in one file
//! encrypted with DPAPI for this Windows user: desktop tokens and the shop's
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

#[cfg(windows)]
fn protect(data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
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

#[cfg(not(windows))]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("兔啃目前只支持 Windows".into())
}

pub fn load(path: &Path) -> Result<Vault, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&protect(&bytes, true)?)
            .map_err(|_| "账户文件损坏，请重新登录".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vault::default()),
        Err(_) => Err("无法读取账户文件".into()),
    }
}

pub fn save(path: &Path, v: &Vault) -> Result<(), String> {
    let dir = path.parent().ok_or("账户目录无效")?;
    std::fs::create_dir_all(dir).map_err(|_| "无法创建账户目录")?;
    let plain = serde_json::to_vec(v).map_err(|_| "无法保存账户")?;
    let data = protect(&plain, false)?;
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

    #[cfg(windows)]
    #[test]
    fn vault_round_trip_is_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let mut v = Vault::default();
        v.upsert(account(7));
        v.active = Some(7);
        save(&path, &v).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("tld_secret_7"));
        assert_eq!(load(&path).unwrap().current().unwrap().user_id, 7);
    }
}
