use md5::{Digest, Md5};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::ZipArchive;

const MD5_URL: &str = "https://raw.githubusercontent.com/blusewill/seer-pc-updater/refs/heads/master/current_version_md5";

const APP_CONFIG_URL: &str = "https://seerdf.61.com.tw/Assets/App/appconfig.json";

const DOWNLOAD_BASE_URL: &str = "http://seerdf.61.com.tw/Assets/App/";

#[derive(Debug, Deserialize)]
struct AppConfig {
    version: String,
    filename: String,
    md5: String,
    size: u64,
}

// 計算檔案 MD5
fn calculate_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();

    // 64 KB buffer，避免在 Stack 上配置過大的陣列
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

fn download_file(
    client: &Client,
    url: &str,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("開始下載更新檔...");
    println!("URL: {}", url);

    let mut response = client.get(url).send()?;

    if !response.status().is_success() {
        return Err(format!("下載失敗，HTTP 狀態碼：{}", response.status()).into());
    }

    let mut file = File::create(destination)?;

    response.copy_to(&mut file)?;

    Ok(())
}

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

fn start_game(seer_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("啟動賽爾號...");

    Command::new(seer_path)
        .current_dir(seer_path.parent().unwrap_or_else(|| Path::new(".")))
        .spawn()?;

    Ok(())
}

fn get_temp_zip_path(filename: &str) -> PathBuf {
    let temp_dir = env::temp_dir();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    temp_dir.join(format!("seer_update_{}_{}", timestamp, filename))
}

fn main() {
    println!("================================");
    println!("       賽爾號更新器");
    println!("================================\n");

    // --------------------------------------------------
    // 取得遊戲資料夾
    // --------------------------------------------------

    let updater_path = match env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("無法取得更新器位置：{}", err);
            return;
        }
    };

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

    let client = match Client::builder().user_agent("Seer-Updater").build() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("建立 HTTP Client 失敗：{}", err);
            return;
        }
    };

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
