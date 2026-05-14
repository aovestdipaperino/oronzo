use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const SWITCHER_KEYCHAIN_SERVICE: &str = "Claude Code-switcher";

const RED: &str = "\x1b[0;31m";
const GREEN: &str = "\x1b[0;32m";
const YELLOW: &str = "\x1b[1;33m";
const CYAN: &str = "\x1b[0;36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const NC: &str = "\x1b[0m";

// ---------------------------------------------------------------------------
// Credential storage — platform-specific
// ---------------------------------------------------------------------------

// macOS: Keychain via `security` CLI

#[cfg(target_os = "macos")]
fn parse_password(combined: &str) -> Option<String> {
    for line in combined.lines() {
        if line.contains("password:") {
            let first = line.find('"')?;
            let last = line.rfind('"')?;
            if last > first {
                return Some(line[first + 1..last].to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn cred_read(service: &str, account: Option<&str>) -> Option<String> {
    let out = if let Some(acct) = account {
        Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", acct, "-g"])
            .output()
            .ok()?
    } else {
        Command::new("security")
            .args(["find-generic-password", "-l", service, "-g"])
            .output()
            .ok()?
    };
    let mut combined = String::with_capacity(out.stderr.len() + out.stdout.len());
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    combined.push_str(&String::from_utf8_lossy(&out.stdout));
    parse_password(&combined)
}

#[cfg(target_os = "macos")]
fn cred_write(service: &str, account: &str, password: &str) -> bool {
    Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            account,
            "-s",
            service,
            "-w",
            password,
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(target_os = "macos")]
fn cred_delete(service: &str, account: Option<&str>) {
    if let Some(acct) = account {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", acct])
            .output();
    } else {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-l", service])
            .output();
    }
}

// Linux: libsecret via `secret-tool` CLI

#[cfg(target_os = "linux")]
fn cred_read(service: &str, account: Option<&str>) -> Option<String> {
    let mut args = vec!["lookup", "service", service];
    if let Some(acct) = account {
        args.push("account");
        args.push(acct);
    }
    let out = Command::new("secret-tool").args(&args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let pw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pw.is_empty() { None } else { Some(pw) }
}

#[cfg(target_os = "linux")]
fn cred_write(service: &str, account: &str, password: &str) -> bool {
    let child = Command::new("secret-tool")
        .args([
            "store", "--label", service, "service", service, "account", account,
        ])
        .stdin(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            if let Some(ref mut stdin) = c.stdin {
                let _ = stdin.write_all(password.as_bytes());
            }
            c.wait().is_ok_and(|s| s.success())
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn cred_delete(service: &str, account: Option<&str>) {
    let mut args = vec!["clear", "service", service];
    if let Some(acct) = account {
        args.push("account");
        args.push(acct);
    }
    let _ = Command::new("secret-tool").args(&args).output();
}

// Windows: Credential Manager via PowerShell P/Invoke

#[cfg(target_os = "windows")]
const WIN_CRED_CS: &str = r#"
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct CREDENTIAL {
    public int Flags;
    public int Type;
    public string TargetName;
    public string Comment;
    public long LastWritten;
    public int CredentialBlobSize;
    public IntPtr CredentialBlob;
    public int Persist;
    public int AttributeCount;
    public IntPtr Attributes;
    public string TargetAlias;
    public string UserName;
}

public class CredHelper {
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredRead(string target, int type, int flags, out IntPtr cred);
    [DllImport("advapi32")]
    static extern void CredFree(IntPtr cred);
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredWrite(ref CREDENTIAL cred, int flags);
    [DllImport("advapi32", CharSet = CharSet.Unicode)]
    static extern bool CredDelete(string target, int type, int flags);

    public static string Read(string target) {
        IntPtr ptr;
        if (!CredRead(target, 1, 0, out ptr)) return null;
        CREDENTIAL c = (CREDENTIAL)Marshal.PtrToStructure(ptr, typeof(CREDENTIAL));
        byte[] bytes = new byte[c.CredentialBlobSize];
        Marshal.Copy(c.CredentialBlob, bytes, 0, c.CredentialBlobSize);
        CredFree(ptr);
        return Encoding.Unicode.GetString(bytes);
    }

    public static bool Write(string target, string user, string pass) {
        byte[] bytes = Encoding.Unicode.GetBytes(pass);
        CREDENTIAL c = new CREDENTIAL();
        c.Type = 1;
        c.TargetName = target;
        c.UserName = user;
        c.CredentialBlobSize = bytes.Length;
        c.CredentialBlob = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, c.CredentialBlob, bytes.Length);
        c.Persist = 2;
        bool ok = CredWrite(ref c, 0);
        Marshal.FreeHGlobal(c.CredentialBlob);
        return ok;
    }

    public static bool Delete(string target) {
        return CredDelete(target, 1, 0);
    }
}
"#;

#[cfg(target_os = "windows")]
fn win_target(service: &str, account: Option<&str>) -> String {
    match account {
        Some(acct) => format!("{service}/{acct}"),
        None => service.to_string(),
    }
}

#[cfg(target_os = "windows")]
fn win_escape(s: &str) -> String {
    s.replace("'", "''")
}

#[cfg(target_os = "windows")]
fn run_ps(script: &str) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(target_os = "windows")]
fn win_add_type() -> String {
    format!("Add-Type -TypeDefinition @'\n{WIN_CRED_CS}\n'@\n")
}

#[cfg(target_os = "windows")]
fn cred_read(service: &str, account: Option<&str>) -> Option<String> {
    let target = win_target(service, account);
    let script = format!(
        "{}$r = [CredHelper]::Read('{}'); if ($r -ne $null) {{ Write-Output $r }} else {{ exit 1 }}",
        win_add_type(),
        win_escape(&target),
    );
    run_ps(&script)
}

#[cfg(target_os = "windows")]
fn cred_write(service: &str, account: &str, password: &str) -> bool {
    let target = win_target(service, Some(account));
    let script = format!(
        "{}[CredHelper]::Write('{}', '{}', '{}'); exit 0",
        win_add_type(),
        win_escape(&target),
        win_escape(account),
        win_escape(password),
    );
    run_ps(&script).is_some() || {
        // run_ps returns None on empty stdout, but write may succeed with no output
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();
        out.is_ok_and(|o| o.status.success())
    }
}

#[cfg(target_os = "windows")]
fn cred_delete(service: &str, account: Option<&str>) {
    let target = win_target(service, account);
    let script = format!(
        "{}[CredHelper]::Delete('{}')",
        win_add_type(),
        win_escape(&target),
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output();
}

// ---------------------------------------------------------------------------
// High-level credential operations (platform-agnostic)
// ---------------------------------------------------------------------------

fn get_system_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

fn get_keychain_credentials() -> Option<String> {
    cred_read(KEYCHAIN_SERVICE, None)
}

fn set_keychain_credentials(credentials: &str) -> bool {
    cred_delete(KEYCHAIN_SERVICE, None);
    cred_write(KEYCHAIN_SERVICE, &get_system_user(), credentials)
}

fn get_profile_credentials(email: &str) -> Option<String> {
    cred_read(SWITCHER_KEYCHAIN_SERVICE, Some(email))
}

fn set_profile_credentials(email: &str, credentials: &str) -> bool {
    cred_delete(SWITCHER_KEYCHAIN_SERVICE, Some(email));
    cred_write(SWITCHER_KEYCHAIN_SERVICE, email, credentials)
}

// ---------------------------------------------------------------------------
// Paths, profile, and shared logic
// ---------------------------------------------------------------------------

struct Paths {
    switcher_dir: PathBuf,
    accounts_dir: PathBuf,
    claude_json: PathBuf,
}

impl Paths {
    fn resolve() -> io::Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine home directory",
            )
        })?;
        let switcher_dir = home.join(".claude-switcher");
        let accounts_dir = switcher_dir.join("accounts");
        let claude_json = home.join(".claude.json");
        Ok(Self {
            switcher_dir,
            accounts_dir,
            claude_json,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Profile {
    email: String,
    #[serde(default, rename = "displayName")]
    display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    alias: String,
    #[serde(default, rename = "oauthAccount")]
    oauth_account: Value,
    #[serde(default, rename = "userID")]
    user_id: Value,
}

fn setup(paths: &Paths) -> io::Result<()> {
    fs::create_dir_all(&paths.accounts_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.switcher_dir, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&paths.accounts_dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn read_claude_json(paths: &Paths) -> io::Result<Value> {
    let bytes = fs::read(&paths.claude_json)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_claude_json(paths: &Paths, data: &Value) -> io::Result<()> {
    let tmp = paths.claude_json.with_extension("json.tmp");
    let pretty = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    fs::write(&tmp, pretty)?;
    fs::rename(&tmp, &paths.claude_json)?;
    Ok(())
}

fn get_current_account(paths: &Paths) -> Value {
    read_claude_json(paths)
        .ok()
        .and_then(|d| d.get("oauthAccount").cloned())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

fn account_filename(accounts_dir: &Path, email: &str) -> PathBuf {
    let safe = email.replace('@', "_at_").replace(['.', '+'], "_");
    accounts_dir.join(format!("{safe}.json"))
}

fn list_saved_accounts(accounts_dir: &Path) -> Vec<Profile> {
    let mut accounts = Vec::new();
    let Ok(entries) = fs::read_dir(accounts_dir) else {
        return accounts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(profile) = serde_json::from_str::<Profile>(&content)
        {
            accounts.push(profile);
        }
    }
    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    accounts
}

fn name_or_email<'a>(name: &'a str, email: &'a str) -> &'a str {
    if name.is_empty() { email } else { name }
}

fn save_current_account(paths: &Paths, alias: Option<&str>) {
    let account = get_current_account(paths);
    let email = account
        .get("emailAddress")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if email.is_empty() {
        eprintln!("{RED}Error: no account currently logged in.{NC}");
        std::process::exit(1);
    }

    let Some(credentials) = get_keychain_credentials() else {
        eprintln!("{RED}Error: could not read credentials from credential store.{NC}");
        #[cfg(target_os = "linux")]
        eprintln!("{DIM}Ensure secret-tool is installed (libsecret-tools).{NC}");
        std::process::exit(1);
    };

    if !set_profile_credentials(&email, &credentials) {
        eprintln!("{RED}Error: could not save credentials.{NC}");
        std::process::exit(1);
    }

    let claude = match read_claude_json(paths) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{RED}Error reading {}: {e}{NC}",
                paths.claude_json.display()
            );
            std::process::exit(1);
        }
    };

    let user_id = claude
        .get("userID")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let display_name = account
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let profile = Profile {
        email: email.clone(),
        display_name: display_name.clone(),
        alias: alias.unwrap_or("").to_string(),
        oauth_account: account,
        user_id,
    };

    let path = account_filename(&paths.accounts_dir, &email);
    let body = match serde_json::to_string_pretty(&profile) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{RED}Error serializing profile: {e}{NC}");
            std::process::exit(1);
        }
    };
    if let Err(e) = fs::write(&path, body) {
        eprintln!("{RED}Error writing {}: {e}{NC}", path.display());
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    let name = name_or_email(&display_name, &email);
    if let Some(alias) = alias {
        println!("{GREEN}Saved:{NC} {BOLD}{email}{NC} ({name}) as {CYAN}{alias}{NC}");
    } else {
        println!("{GREEN}Saved:{NC} {BOLD}{email}{NC} ({name})");
    }
}

fn switch_to_account(paths: &Paths, profile: &Profile) {
    let email = profile.email.as_str();
    let Some(credentials) = get_profile_credentials(email) else {
        eprintln!("{RED}Error: no credentials found for {email}.{NC}");
        std::process::exit(1);
    };

    if !set_keychain_credentials(&credentials) {
        eprintln!("{RED}Error: could not update credential store.{NC}");
        std::process::exit(1);
    }

    let mut claude = match read_claude_json(paths) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "{RED}Error reading {}: {e}{NC}",
                paths.claude_json.display()
            );
            std::process::exit(1);
        }
    };
    if let Some(obj) = claude.as_object_mut() {
        obj.insert("oauthAccount".into(), profile.oauth_account.clone());
        obj.insert("userID".into(), profile.user_id.clone());
    }
    if let Err(e) = write_claude_json(paths, &claude) {
        eprintln!(
            "{RED}Error writing {}: {e}{NC}",
            paths.claude_json.display()
        );
        std::process::exit(1);
    }

    let name = name_or_email(&profile.display_name, email);
    println!("{GREEN}Switched to:{NC} {BOLD}{email}{NC} ({name})");
    println!("{DIM}Restart Claude Code if it is already running.{NC}");
}

fn cmd_list(paths: &Paths) {
    let saved = list_saved_accounts(&paths.accounts_dir);
    let current_account = get_current_account(paths);
    let current_email = current_account
        .get("emailAddress")
        .and_then(Value::as_str)
        .unwrap_or("");

    if saved.is_empty() {
        println!(
            "{YELLOW}No accounts saved. Run{NC} {BOLD}oronzo account-save{NC} {YELLOW}to save the current account.{NC}"
        );
        return;
    }

    println!("\n{BOLD}Saved accounts:{NC}");
    for acc in &saved {
        let marker = if acc.email == current_email {
            format!("  {GREEN}<- active{NC}")
        } else {
            String::new()
        };
        let alias_tag = if acc.alias.is_empty() {
            String::new()
        } else {
            format!(" [{CYAN}{}{NC}]", acc.alias)
        };
        println!(
            "  - {BOLD}{}{NC} ({}){alias_tag}{marker}",
            acc.email, acc.display_name
        );
    }
    println!();
}

fn print_setup_hint(current_email: &str, saved: &[Profile]) {
    if !saved.iter().any(|a| a.email == current_email) {
        println!(
            "  {DIM}Tip: run {BOLD}oronzo account-save{NC}{DIM} to save the current account.{NC}"
        );
    }
    println!();
}

fn read_line() -> Option<String> {
    let mut buf = String::new();
    let stdin = io::stdin();
    match stdin.lock().read_line(&mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(buf.trim().to_string()),
    }
}

fn interactive_switch(paths: &Paths) {
    let current = get_current_account(paths);
    let current_email = current
        .get("emailAddress")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let current_display = current
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    println!("\n{BOLD}Claude Code — Account Switcher{NC}");
    println!("{}", "-".repeat(42));

    let saved = list_saved_accounts(&paths.accounts_dir);

    if current_email.is_empty() {
        println!("  {YELLOW}No account currently active{NC}");
    } else {
        let name = name_or_email(&current_display, &current_email);
        let alias_tag = saved
            .iter()
            .find(|a| a.email == current_email)
            .filter(|a| !a.alias.is_empty())
            .map(|a| format!(" [{CYAN}{}{NC}]", a.alias))
            .unwrap_or_default();
        println!("  Active:  {CYAN}{BOLD}{current_email}{NC} ({name}){alias_tag}");
    }
    let others: Vec<&Profile> = saved.iter().filter(|a| a.email != current_email).collect();

    println!("\n  Other saved accounts:");
    if others.is_empty() {
        println!("  {DIM}None.{NC}");
        println!();
        print_setup_hint(&current_email, &saved);
        return;
    }

    for (i, acc) in others.iter().enumerate() {
        let n = i + 1;
        let name = name_or_email(&acc.display_name, &acc.email);
        let alias_tag = if acc.alias.is_empty() {
            String::new()
        } else {
            format!(" [{CYAN}{}{NC}]", acc.alias)
        };
        println!("  {BOLD}[{n}]{NC} {} ({name}){alias_tag}", acc.email);
    }
    println!();
    print_setup_hint(&current_email, &saved);

    if others.len() == 1 {
        let target = others[0];
        print!("  Switch to {BOLD}{}{NC}? [y/N] ", target.email);
        let _ = io::stdout().flush();
        let Some(choice) = read_line() else {
            println!("\n  No changes made.");
            return;
        };
        let choice = choice.to_lowercase();
        if choice == "y" || choice == "yes" {
            switch_to_account(paths, target);
            return;
        }
        println!("  No changes made.");
        return;
    }

    print!("  Select [1-{}] or Enter to cancel: ", others.len());
    let _ = io::stdout().flush();
    let Some(choice) = read_line() else {
        println!("\n  No changes made.");
        return;
    };
    if choice.is_empty() {
        println!("  No changes made.");
        return;
    }
    match choice.parse::<usize>() {
        Ok(n) if n >= 1 && n <= others.len() => switch_to_account(paths, others[n - 1]),
        _ => {
            eprintln!("{RED}  Invalid selection.{NC}");
            std::process::exit(1);
        }
    }
}

pub fn run(args: &[String]) {
    let paths = match Paths::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{RED}Error: {e}{NC}");
            std::process::exit(1);
        }
    };
    if let Err(e) = setup(&paths) {
        eprintln!("{RED}Error setting up directories: {e}{NC}");
        std::process::exit(1);
    }

    match args[0].as_str() {
        "account-switch" => interactive_switch(&paths),
        "account-save" => save_current_account(&paths, args.get(1).map(String::as_str)),
        "account-list" => cmd_list(&paths),
        "account-use" => {
            let Some(target) = args.get(1) else {
                eprintln!("{RED}Usage: oronzo account-use <email|alias>{NC}");
                std::process::exit(1);
            };
            let saved = list_saved_accounts(&paths.accounts_dir);
            if let Some(profile) = saved
                .iter()
                .find(|a| a.email == *target || (!a.alias.is_empty() && a.alias == *target))
            {
                switch_to_account(&paths, profile);
            } else {
                eprintln!(
                    "{RED}Account '{target}' not found. Run 'oronzo account-list' to see saved accounts.{NC}"
                );
                std::process::exit(1);
            }
        }
        _ => unreachable!(),
    }
}
