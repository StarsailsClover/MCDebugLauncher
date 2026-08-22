// Microsoft (Xbox Live) account login via OAuth Device Code flow (Alpha 7).
//
// The flow is designed for headless/CLI use: MDL prints a short user code and
// a URL (https://microsoft.com/link); the user enters the code on any device.
// MDL polls the token endpoint until authorization completes, then walks the
// Xbox -> XSTS -> Minecraft chain to obtain a Minecraft access token and
// profile (UUID + name + skins).
//
// Tokens are cached under <data>/accounts/<uuid>.json and refreshed on
// demand. Skin retrieval is provided both via sessionserver textures and
// direct mc-heads avatar URLs.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Public Azure app registration client id shared by open-source launchers
/// (PrismLauncher). Public clients need no secret for the device flow.
pub const CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92095eb";
const DEVICECODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SESSIONSERVER_URL: &str = "https://sessionserver.mojang.com/session/minecraft/profile";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default = "default_expires")]
    expires_in: u64,
}

fn default_interval() -> u64 {
    5
}
fn default_expires() -> u64 {
    900
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftAccount {
    pub uuid: String,
    pub username: String,
    pub access_token: String,
    pub refresh_token: String,
    pub saved_at: u64,
    #[serde(default)]
    pub skin_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XboxAuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: serde_json::Value,
}

fn accounts_dir() -> Result<PathBuf> {
    let dir = crate::util::paths::get_data_dir()?.join("accounts");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn list_accounts() -> Vec<MinecraftAccount> {
    let dir = match accounts_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(e.path()) {
                    if let Ok(acc) = serde_json::from_str::<MinecraftAccount>(&content) {
                        out.push(acc);
                    }
                }
            }
        }
    }
    out
}

/// Begin device flow and block (polling) until the user authorizes.
pub async fn login_interactive() -> Result<MinecraftAccount> {
    let client = crate::util::http::create_http_client()?;

    let form = format!(
        "client_id={}&scope=XboxLive.signin%20offline_access",
        CLIENT_ID
    );
    let dc: DeviceCodeResponse = client
        .post(DEVICECODE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .context("Failed to request device code")?
        .json()
        .await
        .context("Failed to parse device code response")?;

    println!("====================================================");
    println!("  To sign in, open {} in a browser", dc.verification_uri);
    println!("  and enter the code:  {}", dc.user_code);
    println!("  (Waiting up to {}s for authorization...)", dc.expires_in);
    println!("====================================================");

    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(dc.expires_in.max(60));
    let mut interval = dc.interval.max(3);

    let ms_token = loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Device code expired before authorization completed");
        }
        let body = format!(
            "grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id={}&device_code={}",
            CLIENT_ID, dc.device_code
        );
        let resp: TokenResponse = client
            .post(TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .context("Failed to poll token endpoint")?
            .json()
            .await
            .context("Failed to parse token response")?;
        match resp.error.as_deref() {
            None => break (resp.access_token, resp.refresh_token),
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 2;
                continue;
            }
            Some(other) => anyhow::bail!("Microsoft auth error: {}", other),
        }
    };
    // v26.2-alpha.6: keep the refresh token so `mdl account refresh` can
    // mint new access tokens without re-running the device flow.
    let (ms_token, ms_refresh_token) = ms_token;

    // Xbox Live
    let xbl: XboxAuthResponse = client
        .post(XBL_URL)
        .json(&serde_json::json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={}", ms_token) },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .context("Xbox Live auth failed")?
        .json()
        .await?;
    let uhs = xbl.display_claims["xui"][0]["uhs"]
        .as_str()
        .context("Missing XBL user hash")?
        .to_string();

    // XSTS
    let xsts: XboxAuthResponse = client
        .post(XSTS_URL)
        .json(&serde_json::json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .context("XSTS auth failed")?
        .json()
        .await?;

    // Minecraft login
    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
    }
    let mc: McLogin = client
        .post(MC_LOGIN_URL)
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={};{}", uhs, xsts.token)
        }))
        .send()
        .await
        .context("Minecraft login failed")?
        .json()
        .await
        .context("Failed to parse Minecraft login")?;

    // Profile
    let profile: serde_json::Value = client
        .get(MC_PROFILE_URL)
        .header("Authorization", format!("Bearer {}", mc.access_token))
        .send()
        .await
        .context("Failed to fetch Minecraft profile")?
        .json()
        .await?;
    let uuid = profile["id"].as_str().context("Profile missing id")?.to_string();
    let username = profile["name"]
        .as_str()
        .context("Profile missing name")?
        .to_string();
    let skin_url = profile["skins"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(|s| s["url"].as_str())
        .map(|s| s.to_string());

    // v26.2-alpha.6: persist the Microsoft refresh token so accounts can be
    // renewed via `mdl account refresh` instead of re-running device flow.
    let account = MinecraftAccount {
        uuid: uuid.clone(),
        username,
        access_token: mc.access_token,
        refresh_token: ms_refresh_token,
        saved_at: now_secs(),
        skin_url,
    };
    save_account(&account)?;
    Ok(account)
}

/// Refresh a Minecraft account's access token using its stored Microsoft
/// refresh token (v26.2-alpha.6). Updates the saved account on success.
/// Returns an error when the account has no refresh token (legacy cache
/// from before v26.2-alpha.6 — re-login required).
pub async fn refresh_account(acc: &MinecraftAccount) -> Result<MinecraftAccount> {
    if acc.refresh_token.is_empty() {
        anyhow::bail!(
            "Account '{}' has no refresh token (cached before v26.2-alpha.6). Run `mdl account login` again.",
            acc.username
        );
    }
    let client = crate::util::http::create_http_client()?;
    let resp: TokenResponse = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&client_id={}&refresh_token={}",
            CLIENT_ID, acc.refresh_token
        ))
        .send()
        .await
        .context("Failed to call Microsoft token endpoint")?
        .json()
        .await
        .context("Failed to parse token refresh response")?;
    if let Some(err) = &resp.error {
        anyhow::bail!("Microsoft auth error: {} (re-login may be required)", err);
    }

    let mut updated = acc.clone();
    updated.access_token = resp.access_token;
    // Refresh-token rotation: Microsoft may return a new refresh token;
    // keep the old one when it does not.
    if !resp.refresh_token.is_empty() {
        updated.refresh_token = resp.refresh_token;
    }
    updated.saved_at = now_secs();
    save_account(&updated)?;
    Ok(updated)
}

fn save_account(acc: &MinecraftAccount) -> Result<()> {
    let dir = accounts_dir()?;
    let path = dir.join(format!("{}.json", acc.uuid));
    std::fs::write(&path, serde_json::to_string_pretty(acc)?)?;
    Ok(())
}

pub fn find_account(uuid_or_name: &str) -> Option<MinecraftAccount> {
    list_accounts()
        .into_iter()
        .find(|a| a.uuid == uuid_or_name || a.username.eq_ignore_ascii_case(uuid_or_name))
}

/// Fetch the skin texture URL for a UUID via sessionserver.
pub async fn skin_url(uuid: &str) -> Result<String> {
    let client = crate::util::http::create_http_client()?;
    let resp: serde_json::Value = client
        .get(format!("{}/{}?unsigned=false", SESSIONSERVER_URL, uuid))
        .send()
        .await
        .context("Failed to query sessionserver")?
        .json()
        .await?;
    let b64 = resp["properties"][0]["value"]
        .as_str()
        .context("No textures property")?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("Failed to decode textures")?;
    let textures: serde_json::Value = serde_json::from_slice(&decoded)?;
    textures["textures"]["SKIN"]["url"]
        .as_str()
        .map(|s| s.to_string())
        .context("No SKIN url in textures")
}

/// Direct avatar image URL via mc-heads (no auth needed).
pub fn avatar_url(uuid_or_name: &str, size: u32) -> String {
    format!("https://mc-heads.net/avatar/{}/{}", uuid_or_name, size)
}

/// Download the skin PNG for a UUID into `dest`.
pub async fn download_skin(uuid: &str, dest: &Path) -> Result<()> {
    let url = skin_url(uuid).await?;
    crate::version::downloader::download_file(&url, dest, None).await?;
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avatar_url() {
        assert_eq!(avatar_url("Notch", 64), "https://mc-heads.net/avatar/Notch/64");
    }
}
