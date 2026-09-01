#![windows_subsystem = "windows"]

use chrono::{FixedOffset, Timelike, Utc};
use eframe::egui;
use serde::Deserialize;
use std::path::PathBuf;

// DeepSeek-chat (V3) 价格, CNY / 每百万 tokens
const OUTPUT_PRICE: f64 = 8.0;
const INPUT_PRICE: f64 = 2.0;
const OFFPEAK_DISCOUNT: f64 = 0.5;

#[derive(Deserialize)]
struct BalanceResponse {
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Deserialize, Clone)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
}

struct App {
    api_key: String,
    balance: Option<BalanceInfo>,
    baseline: Option<f64>,
    error: Option<String>,
    initialized: bool,
    last_refresh: std::time::Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            api_key: load_key(),
            balance: None,
            baseline: load_baseline(),
            error: None,
            initialized: false,
            last_refresh: std::time::Instant::now(),
        }
    }
}

fn key_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default())
        .join("deepseek-balance")
}

fn load_key() -> String {
    std::fs::read_to_string(key_dir().join("api_key.txt"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn save_key(key: &str) {
    let dir = key_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("api_key.txt"), key);
    }
}

fn load_baseline() -> Option<f64> {
    std::fs::read_to_string(key_dir().join("baseline.txt"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
}

fn save_baseline(v: f64) {
    let dir = key_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("baseline.txt"), v.to_string());
    }
}

fn install_chinese_font(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\Deng.ttf",
        "C:\\Windows\\Fonts\\simfang.ttf",
        "C:\\Windows\\Fonts\\simkai.ttf",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("chinese".to_owned(), egui::FontData::from_owned(bytes));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, "chinese".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

fn beijing_time() -> chrono::DateTime<FixedOffset> {
    let tz = FixedOffset::east_opt(8 * 3600).unwrap();
    Utc::now().with_timezone(&tz)
}

fn is_off_peak() -> bool {
    let t = beijing_time();
    let minutes = t.hour() as u32 * 60 + t.minute() as u32;
    // 谷值时段: 16:30 - 24:00 及 00:00 - 00:30 (北京时间)
    minutes >= 16 * 60 + 30 || minutes < 30
}

fn query_balance(api_key: &str) -> Result<BalanceInfo, String> {
    let resp = ureq::get("https://api.deepseek.com/user/balance")
        .set("Authorization", &format!("Bearer {}", api_key))
        .call()
        .map_err(|e| e.to_string())?;
    let body: BalanceResponse = resp.into_json().map_err(|e| e.to_string())?;
    body.balance_infos
        .into_iter()
        .find(|b| b.currency == "CNY")
        .ok_or_else(|| "未找到 CNY 余额".to_string())
}

fn fmt_tokens(n: f64) -> String {
    if n >= 1_000_000_000_000.0 {
        format!("{:.2} 万亿", n / 1_000_000_000_000.0)
    } else if n >= 100_000_000.0 {
        format!("{:.2} 亿", n / 100_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.2} 万", n / 10_000.0)
    } else {
        format!("{:.0}", n)
    }
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}****{tail}")
}

impl App {
    fn refresh(&mut self) {
        self.last_refresh = std::time::Instant::now();
        self.error = None;
        let key = self.api_key.trim().to_string();
        match query_balance(&key) {
            Ok(info) => {
                if let Ok(cur) = info.total_balance.parse::<f64>() {
                    self.update_baseline(cur);
                }
                self.balance = Some(info);
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn update_baseline(&mut self, current: f64) {
        match self.baseline {
            None => {
                self.baseline = Some(current);
                save_baseline(current);
            }
            Some(b) if current > b => {
                self.baseline = Some(current);
                save_baseline(current);
            }
            _ => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initialized {
            self.initialized = true;
            if !self.api_key.trim().is_empty() {
                self.refresh();
            }
        }

        if self.balance.is_some() {
            let elapsed = self.last_refresh.elapsed();
            if elapsed >= std::time::Duration::from_secs(30) {
                self.refresh();
            }
            let remain = std::time::Duration::from_secs(30)
                .saturating_sub(self.last_refresh.elapsed());
            ctx.request_repaint_after(remain);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("DeepSeek 余额查询");

        if self.balance.is_none() {
            ui.horizontal(|ui| {
                ui.label("API Key:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.api_key)
                        .password(true)
                        .desired_width(300.0),
                );
            });

            if ui.button("查询").clicked() {
                let key = self.api_key.trim().to_string();
                save_key(&key);
                self.refresh();
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(format!("已登录, API Key: {}...", mask_key(&self.api_key)));
                if ui.button("刷新").clicked() {
                    self.refresh();
                }
                if ui.button("退出登录").clicked() {
                    self.api_key.clear();
                    self.balance = None;
                    self.error = None;
                    save_key("");
                }
            });
        }

        if let Some(err) = &self.error {
            ui.colored_label(egui::Color32::RED, format!("错误: {err}"));
        }

        if let Some(info) = &self.balance {
            let balance: f64 = info.total_balance.parse().unwrap_or(0.0);
            let off_peak = is_off_peak();
            let (input_p, output_p) = if off_peak {
                (INPUT_PRICE * OFFPEAK_DISCOUNT, OUTPUT_PRICE * OFFPEAK_DISCOUNT)
            } else {
                (INPUT_PRICE, OUTPUT_PRICE)
            };

            ui.separator();
            ui.heading(format!("余额: ¥{:.2}", balance));
            if let Some(base) = self.baseline {
                let spent = (base - balance).max(0.0);
                ui.label(format!("累计已烧: ¥{:.2}", spent));
            }
            if off_peak {
                ui.colored_label(egui::Color32::GREEN, "当前处于【谷值时段】价格五折");
            } else {
                ui.colored_label(egui::Color32::LIGHT_RED, "当前处于【峰值时段】");
            }
            ui.separator();
            ui.label(format!(
                "预计可输出 token: {} (约 ¥{:.2}/百万)",
                fmt_tokens(balance / output_p * 1_000_000.0),
                output_p
            ));
            ui.label(format!(
                "预计可输入 token: {} (约 ¥{:.2}/百万)",
                fmt_tokens(balance / input_p * 1_000_000.0),
                input_p
            ));
        }
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(egui::IconData::default()),
        ..Default::default()
    };
    eframe::run_native(
        "DeepSeek 余额查询",
        options,
        Box::new(|cc| {
            install_chinese_font(&cc.egui_ctx);
            Ok(Box::new(App::default()))
        }),
    )
}
