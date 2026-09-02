use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

// ============================================================
// 遊戲更新資訊
// ============================================================

const MD5_URL: &str = "https://raw.githubusercontent.com/blusewill/seer-pc-updater/refs/heads/master/current_version_md5";

const APP_CONFIG_URL: &str = "https://seerdf.61.com.tw/Assets/App/appconfig.json";

const DOWNLOAD_BASE_URL: &str = "http://seerdf.61.com.tw/Assets/App/";

// ============================================================
// Updater 自我更新
// ============================================================

const UPDATER_SHA256_URL: &str =
    "https://github.com/blusewill/seer-pc-updater/releases/latest/download/updater.exe.sha256";

const UPDATER_DOWNLOAD_URL: &str =
    "https://github.com/blusewill/seer-pc-updater/releases/latest/download/updater.exe";

// ============================================================
// AppConfig
// ============================================================

#[derive(Debug, Deserialize)]
struct AppConfig {
    version: String,
    filename: String,
    md5: String,
    size: u64,
}

// ============================================================
// 計算檔案 MD5
// ============================================================

fn calculate_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = md5::Md5::new();

    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

// ============================================================
// 計算檔案 SHA-256
// ============================================================

fn calculate_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

// ============================================================
// 下載檔案
// ============================================================

fn download_file(
    client: &Client,
    url: &str,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("開始下載...");
    println!("URL: {}", url);

    let mut response = client.get(url).send()?;

    if !response.status().is_success() {
        return Err(format!("下載失敗，HTTP 狀態碼：{}", response.status()).into());
    }

    let mut file = File::create(destination)?;

    response.copy_to(&mut file)?;

    Ok(())
}

// ============================================================
// 取得遠端 SHA-256
//
// GitHub .sha256 常見格式：
//
// abcdef1234567890...  updater.exe
//
// 也支援只有：
//
// abcdef1234567890...
// ============================================================

fn parse_sha256(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // 取第一個空白以前的內容
        let hash = line.split_whitespace().next().unwrap_or("");

        if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hash.to_lowercase());
        }
    }

    None
}

// ============================================================
// 取得暫存檔案名稱
// ============================================================

fn get_temp_file_path(prefix: &str, extension: &str) -> PathBuf {
    let temp_dir = env::temp_dir();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    temp_dir.join(format!("{}_{}.{}", prefix, timestamp, extension))
}

// ============================================================
// 自我更新
// ============================================================

fn check_for_updater_update(
    client: &Client,
    updater_path: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    println!("================================");
    println!("       檢查更新器版本");
    println!("================================\n");

    println!("目前更新器：{}", updater_path.display());

    // --------------------------------------------------------
    // 計算目前 updater.exe SHA-256
    // --------------------------------------------------------

    let local_sha256 = calculate_sha256(updater_path)?.to_lowercase();

    println!("本機 SHA-256：{}", local_sha256);

    // --------------------------------------------------------
    // 取得 Release 上的 updater.exe.sha256
    // --------------------------------------------------------

    println!("\n取得 GitHub Release SHA-256...");

    let response = client.get(UPDATER_SHA256_URL).send()?;

    if !response.status().is_success() {
        return Err(format!("取得 updater.exe.sha256 失敗：HTTP {}", response.status()).into());
    }

    let sha256_text = response.text()?;

    let remote_sha256 = parse_sha256(&sha256_text).ok_or("無法解析 updater.exe.sha256")?;

    println!("Release SHA-256：{}", remote_sha256);

    // --------------------------------------------------------
    // SHA-256 相同
    // --------------------------------------------------------

    if local_sha256 == remote_sha256 {
        println!("\n更新器已經是最新版本！");
        return Ok(false);
    }

    // --------------------------------------------------------
    // SHA-256 不同
    // --------------------------------------------------------

    println!("\n發現新的更新器版本！");
    println!("準備下載新的 updater.exe...");

    let new_updater = get_temp_file_path("seer_updater_new", "exe");

    // --------------------------------------------------------
    // 下載新版 updater.exe
    // --------------------------------------------------------

    if let Err(err) = download_file(client, UPDATER_DOWNLOAD_URL, &new_updater) {
        let _ = fs::remove_file(&new_updater);

        return Err(format!("下載新版 updater.exe 失敗：{}", err).into());
    }

    // --------------------------------------------------------
    // 驗證下載的 updater.exe
    // --------------------------------------------------------

    println!("\n正在驗證新版 updater.exe...");

    let downloaded_sha256 = calculate_sha256(&new_updater)?.to_lowercase();

    println!("下載檔 SHA-256：{}", downloaded_sha256);

    if downloaded_sha256 != remote_sha256 {
        let _ = fs::remove_file(&new_updater);

        return Err("新版 updater.exe SHA-256 驗證失敗！".into());
    }

    println!("新版 updater.exe 驗證成功！");

    // --------------------------------------------------------
    // 建立自我更新 BAT
    // --------------------------------------------------------

    let current_pid = std::process::id();

    let bat_path = get_temp_file_path("seer_updater_replace", "bat");

    let current_updater = updater_path
        .canonicalize()
        .unwrap_or_else(|_| updater_path.to_path_buf());

    let new_updater = new_updater
        .canonicalize()
        .unwrap_or_else(|_| new_updater.clone());

    let bat_content = format!(
        r#"@echo off
setlocal

echo Waiting for old updater to exit...

:wait
tasklist /FI "PID eq {pid}" 2>NUL | find "{pid}" >NUL
if not errorlevel 1 (
    timeout /t 1 /nobreak >NUL
    goto wait
)

echo Replacing updater...

copy /Y "{new_updater}" "{current_updater}" >NUL

if errorlevel 1 (
    echo Failed to replace updater.
    del /Q "{new_updater}" >NUL 2>&1
    del /Q "%~f0" >NUL 2>&1
    exit /b 1
)

echo Starting updated updater...

start "" "{current_updater}"

del /Q "{new_updater}" >NUL 2>&1
del /Q "%~f0" >NUL 2>&1

exit /b 0
"#,
        pid = current_pid,
        new_updater = new_updater.display(),
        current_updater = current_updater.display(),
    );

    let mut bat_file = File::create(&bat_path)?;
    bat_file.write_all(bat_content.as_bytes())?;
    bat_file.flush()?;

    // --------------------------------------------------------
    // 啟動 BAT
    // --------------------------------------------------------

    println!("\n正在準備更新器...");
    println!("舊版 updater 即將結束。");

    Command::new("cmd")
        .args(["/C", "start", "", bat_path.to_string_lossy().as_ref()])
        .spawn()?;

    // --------------------------------------------------------
    // 告訴 main：需要結束
    // --------------------------------------------------------

    Ok(true)
}

// ============================================================
// 解壓縮 ZIP
// ============================================================

fn extract_zip(zip_path: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("正在解壓縮更新檔...");

    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;

        let relative_path = match entry.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };

        let output_path = destination.join(relative_path);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        println!("更新：{}", output_path.display());

        let mut output_file = File::create(&output_path)?;
        io::copy(&mut entry, &mut output_file)?;
    }

    Ok(())
}

// ============================================================
// 啟動遊戲
// ============================================================

fn start_game(seer_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("啟動賽爾號...");

    Command::new(seer_path)
        .current_dir(seer_path.parent().unwrap_or_else(|| Path::new(".")))
        .spawn()?;

    Ok(())
}

// ============================================================
// TEMP ZIP
// ============================================================

fn get_temp_zip_path(filename: &str) -> PathBuf {
    let temp_dir = env::temp_dir();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    temp_dir.join(format!("seer_update_{}_{}", timestamp, filename))
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    println!("================================");
    println!("       賽爾號更新器");
    println!("================================\n");

    // --------------------------------------------------
    // 取得 updater.exe 路徑
    // --------------------------------------------------

    let updater_path = match env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("無法取得更新器位置：{}", err);
            return;
        }
    };

    // --------------------------------------------------
    // 建立 HTTP Client
    // --------------------------------------------------

    let client = match Client::builder().user_agent("Seer-Updater").build() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("建立 HTTP Client 失敗：{}", err);
            return;
        }
    };

    // --------------------------------------------------
    // 第一件事情：檢查 updater.exe 自己
    // --------------------------------------------------

    match check_for_updater_update(&client, &updater_path) {
        Ok(true) => {
            // 已經啟動自我更新 BAT
            // 舊 updater 必須結束
            println!("Updater 正在更新，結束目前版本。");
            return;
        }

        Ok(false) => {
            // 已經是最新版
        }

        Err(err) => {
            // 更新器版本檢查失敗時，
            // 不直接阻止遊戲更新。
            //
            // 這樣即使 GitHub 暫時無法連線，
            // 使用者仍然可以啟動舊版 updater。
            eprintln!("\n檢查更新器版本失敗：{}", err);

            eprintln!("將繼續使用目前版本的更新器。\n");
        }
    }

    // --------------------------------------------------
    // 取得遊戲資料夾
    // --------------------------------------------------

    let game_dir = match updater_path.parent() {
        Some(path) => path.to_path_buf(),
        None => {
            eprintln!("無法取得遊戲資料夾");
            return;
        }
    };

    let seer_path = game_dir.join("seer.exe");

    if !seer_path.exists() {
        eprintln!("找不到 seer.exe");
        eprintln!("{}", seer_path.display());
        return;
    }

    println!("遊戲資料夾：{}", game_dir.display());

    // --------------------------------------------------
    // 計算本機 seer.exe MD5
    // --------------------------------------------------

    println!("\n正在檢查目前版本...");

    let local_md5 = match calculate_md5(&seer_path) {
        Ok(md5) => md5.to_lowercase(),
        Err(err) => {
            eprintln!("無法計算 seer.exe MD5：{}", err);
            return;
        }
    };

    println!("本機 MD5：{}", local_md5);

    // --------------------------------------------------
    // 取得 GitHub 上的目前版本 MD5
    // --------------------------------------------------

    let response = match client.get(MD5_URL).send() {
        Ok(response) => response,

        Err(err) => {
            eprintln!("無法取得遠端 MD5：{}", err);
            return;
        }
    };

    if !response.status().is_success() {
        eprintln!("取得遠端 MD5 失敗：HTTP {}", response.status());
        return;
    }

    let remote_md5 = match response.text() {
        Ok(text) => text.trim().to_lowercase(),

        Err(err) => {
            eprintln!("讀取遠端 MD5 失敗：{}", err);
            return;
        }
    };

    println!("遠端 MD5：{}", remote_md5);

    // --------------------------------------------------
    // MD5 相同 → 直接啟動遊戲
    // --------------------------------------------------

    if local_md5 == remote_md5 {
        println!("\n目前已經是最新版本！");
        println!("直接啟動遊戲。\n");

        if let Err(err) = start_game(&seer_path) {
            eprintln!("賽爾號啟動失敗：{}", err);
        }

        return;
    }

    // --------------------------------------------------
    // MD5 不同 → 開始更新
    // --------------------------------------------------

    println!("\n發現新版本！");
    println!("開始取得更新資訊...\n");

    // --------------------------------------------------
    // 取得 appconfig.json
    // --------------------------------------------------

    let config_response = match client.get(APP_CONFIG_URL).send() {
        Ok(response) => response,

        Err(err) => {
            eprintln!("無法取得 appconfig.json：{}", err);
            return;
        }
    };

    if !config_response.status().is_success() {
        eprintln!(
            "取得 appconfig.json 失敗：HTTP {}",
            config_response.status()
        );
        return;
    }

    let config_text = match config_response.text() {
        Ok(text) => text,

        Err(err) => {
            eprintln!("讀取 appconfig.json 失敗：{}", err);
            return;
        }
    };

    let config: AppConfig = match serde_json::from_str(&config_text) {
        Ok(config) => config,

        Err(err) => {
            eprintln!("解析 appconfig.json 失敗：{}", err);
            return;
        }
    };

    println!("最新版本：{}", config.version);
    println!("更新檔案：{}", config.filename);
    println!("更新檔 MD5：{}", config.md5);
    println!("更新檔大小：{} bytes", config.size);

    // --------------------------------------------------
    // 建立下載網址
    // --------------------------------------------------

    let download_url = format!("{}{}", DOWNLOAD_BASE_URL, config.filename);

    println!("\n下載網址：{}", download_url);

    // --------------------------------------------------
    // 下載到 TEMP
    // --------------------------------------------------

    let temp_zip = get_temp_zip_path(&config.filename);

    println!("暫存位置：{}", temp_zip.display());

    if let Err(err) = download_file(&client, &download_url, &temp_zip) {
        eprintln!("下載更新檔失敗：{}", err);

        let _ = fs::remove_file(&temp_zip);

        return;
    }

    // --------------------------------------------------
    // 驗證 ZIP MD5
    // --------------------------------------------------

    println!("\n正在驗證更新檔...");

    let downloaded_md5 = match calculate_md5(&temp_zip) {
        Ok(md5) => md5.to_lowercase(),

        Err(err) => {
            eprintln!("無法計算更新檔 MD5：{}", err);

            let _ = fs::remove_file(&temp_zip);

            return;
        }
    };

    println!("下載檔 MD5：{}", downloaded_md5);

    println!("伺服器 MD5：{}", config.md5);

    if downloaded_md5 != config.md5.to_lowercase() {
        eprintln!("更新檔 MD5 驗證失敗！");

        eprintln!("為避免損壞遊戲檔案，取消更新。");

        let _ = fs::remove_file(&temp_zip);

        return;
    }

    println!("更新檔驗證成功！");

    // --------------------------------------------------
    // 解壓縮並覆蓋遊戲檔案
    // --------------------------------------------------

    if let Err(err) = extract_zip(&temp_zip, &game_dir) {
        eprintln!("解壓縮更新檔失敗：{}", err);

        let _ = fs::remove_file(&temp_zip);

        return;
    }

    println!("\n更新完成！");

    // --------------------------------------------------
    // 清理 TEMP
    // --------------------------------------------------

    if let Err(err) = fs::remove_file(&temp_zip) {
        eprintln!("清理暫存更新檔失敗：{}", err);
    }

    // --------------------------------------------------
    // 啟動遊戲
    // --------------------------------------------------

    if let Err(err) = start_game(&seer_path) {
        eprintln!("賽爾號啟動失敗：{}", err);

        return;
    }

    println!("遊戲已啟動，更新器結束。");
}
