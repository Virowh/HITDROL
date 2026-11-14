use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

struct Cached {
    texture: egui::TextureHandle,
    used: u64,
    bytes: usize,
}
pub struct TextureCache {
    cache: HashMap<PathBuf, Cached>,
    pending: HashSet<PathBuf>,
    failed: HashMap<PathBuf, Failed>,
    tx: mpsc::Sender<PathBuf>,
    rx: mpsc::Receiver<(PathBuf, Result<egui::ColorImage, String>)>,
    clock: u64,
    pub bytes: usize,
}
struct Failed {
    stamp: Option<(u64, SystemTime)>,
    retry: Instant,
}
fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}
impl TextureCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, jobs) = mpsc::channel::<PathBuf>();
        // Keep decoded images bounded even while a minimized window stops draining them.
        let (results, rx) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            for path in jobs {
                let result = decode(&path).map_err(|e| e.to_string());
                if results.send((path, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            cache: HashMap::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            tx,
            rx,
            clock: 0,
            bytes: 0,
        }
    }
    pub fn drain(&mut self, ctx: &egui::Context) {
        while let Ok((path, result)) = self.rx.try_recv() {
            self.pending.remove(&path);
            match result {
                Ok(image) => {
                    let bytes = image.pixels.len() * 4;
                    let texture = ctx.load_texture(
                        path.to_string_lossy(),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.bytes += bytes;
                    self.cache.insert(
                        path,
                        Cached {
                            texture,
                            used: self.clock,
                            bytes,
                        },
                    );
                }
                Err(_) => {
                    let failed = Failed {
                        stamp: stamp(&path),
                        retry: Instant::now() + Duration::from_secs(30),
                    };
                    self.failed.insert(path, failed);
                }
            }
        }
        while self.bytes > 80 * 1024 * 1024 || self.cache.len() > 128 {
            let Some(key) = self
                .cache
                .iter()
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(old) = self.cache.remove(&key) {
                self.bytes -= old.bytes;
            }
        }
    }
    pub fn get(&mut self, path: Option<&PathBuf>) -> Option<(egui::TextureId, egui::Vec2)> {
        let path = path?;
        self.clock += 1;
        if let Some(v) = self.cache.get_mut(path) {
            v.used = self.clock;
            return Some((v.texture.id(), v.texture.size_vec2()));
        }
        let retry = self
            .failed
            .get(path)
            .is_none_or(|f| Instant::now() >= f.retry || stamp(path) != f.stamp);
        if retry && self.pending.insert(path.clone()) {
            self.failed.remove(path);
            let _ = self.tx.send(path.clone());
        }
        None
    }
    pub fn get_any<const N: usize>(
        &mut self,
        paths: [Option<&PathBuf>; N],
    ) -> Option<(egui::TextureId, egui::Vec2, usize)> {
        paths
            .into_iter()
            .enumerate()
            .find_map(|(i, path)| self.get(path).map(|(id, size)| (id, size, i)))
    }
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }
    pub fn invalidate(&mut self, path: &Path) {
        self.failed.remove(path);
        if let Some(old) = self.cache.remove(path) {
            self.bytes -= old.bytes;
        }
    }
}
fn decode(path: &Path) -> Result<egui::ColorImage> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 24 * 1024 * 1024,
        "Изображение слишком большое"
    );
    let bytes = std::fs::read(path)?;
    let rgba = decode_bytes(&bytes)?;
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}
pub(crate) fn decode_bytes(bytes: &[u8]) -> Result<image::RgbaImage> {
    anyhow::ensure!(
        bytes.len() <= 24 * 1024 * 1024,
        "Изображение слишком большое"
    );
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    let image = if image.width() > 1600 || image.height() > 1200 {
        image.thumbnail(1600, 1200)
    } else {
        image
    };
    Ok(image.to_rgba8())
}
#[derive(Debug, Clone, Deserialize)]
pub struct ArtGame {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub release_date: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ArtImage {
    pub id: u64,
    pub url: String,
    pub thumb: String,
}
#[derive(Deserialize)]
struct Response<T> {
    success: bool,
    data: T,
}
pub struct SteamGrid {
    client: reqwest::blocking::Client,
    key: String,
}
impl SteamGrid {
    pub fn new() -> Result<Self> {
        let key =
            crate::platform::read_key()?.context("Добавьте API-ключ SteamGridDB в настройках")?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(concat!("HITDROLLauncher/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client, key })
    }
    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get_url(&format!("https://www.steamgriddb.com/api/v2/{path}"))
    }
    fn get_url<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self.client.get(url).bearer_auth(&self.key).send()?;
        let status = response.status();
        anyhow::ensure!(
            status.as_u16() != 401 && status.as_u16() != 403,
            "SteamGridDB: API-ключ не принят"
        );
        anyhow::ensure!(
            status.as_u16() != 429,
            "SteamGridDB: слишком много запросов, попробуйте позже"
        );
        let data: Response<T> = response.error_for_status()?.json()?;
        anyhow::ensure!(data.success, "SteamGridDB не вернул результат");
        Ok(data.data)
    }
    pub fn search(&self, name: &str) -> Result<Vec<ArtGame>> {
        let encoded: String = name
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        self.get(&format!("search/autocomplete/{encoded}"))
    }
    pub fn steam_game(&self, id: &str) -> Result<ArtGame> {
        anyhow::ensure!(
            id.chars().all(|c| c.is_ascii_digit()),
            "Некорректный Steam ID"
        );
        self.get(&format!("games/steam/{id}"))
    }
    pub fn images(&self, id: u64, hero: bool) -> Result<Vec<ArtImage>> {
        let category = if hero { "heroes" } else { "grids" };
        let sizes = if hero { "" } else { "&dimensions=600x900" };
        self.get(&format!(
            "{category}/game/{id}?types=static&nsfw=false&humor=false{sizes}"
        ))
    }
    pub fn download(&self, url: &str, cache: &Path) -> Result<PathBuf> {
        let url = reqwest::Url::parse(url)?;
        anyhow::ensure!(
            url.scheme() == "https"
                && url
                    .host_str()
                    .is_some_and(|h| h == "steamgriddb.com" || h.ends_with(".steamgriddb.com")),
            "Неподдерживаемый адрес изображения"
        );
        std::fs::create_dir_all(cache)?;
        let path = cache.join(format!("{}.png", crate::model::stable_hash(url.as_str())));
        if path.is_file()
            && std::fs::read(&path)
                .ok()
                .is_some_and(|bytes| decode_bytes(&bytes).is_ok())
        {
            return Ok(path);
        }
        let response = self.client.get(url).send()?.error_for_status()?;
        anyhow::ensure!(
            response.content_length().unwrap_or(0) <= 24 * 1024 * 1024,
            "Изображение слишком большое"
        );
        let mut bytes = Vec::new();
        response
            .take(24 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let image = decode_bytes(&bytes)?;
        let tmp = path.with_extension("tmp");
        image.save_with_format(&tmp, image::ImageFormat::Png)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }
}
pub fn import_image(from: &Path, cache: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(cache)?;
    anyhow::ensure!(
        std::fs::metadata(from)?.len() <= 24 * 1024 * 1024,
        "Изображение слишком большое"
    );
    let bytes = std::fs::read(from)?;
    let image = decode_bytes(&bytes)?;
    use sha2::{Digest, Sha256};
    let path = cache.join(format!("custom-{:x}.png", Sha256::digest(&bytes)));
    image.save(&path)?;
    Ok(path)
}
pub fn normalized_title(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wait_for_images(cache: &mut TextureCache, ctx: &egui::Context) {
        let start = Instant::now();
        while !cache.pending.is_empty() && start.elapsed() < Duration::from_secs(3) {
            cache.drain(ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(cache.pending.is_empty());
    }
    #[test]
    fn failed_image_reloads_when_file_is_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("обложка.png");
        std::fs::write(&path, b"broken").unwrap();
        let ctx = egui::Context::default();
        let mut cache = TextureCache::new(ctx.clone());
        assert!(cache.get(Some(&path)).is_none());
        wait_for_images(&mut cache, &ctx);
        assert!(cache.failed.contains_key(&path));
        image::RgbaImage::from_pixel(30, 45, image::Rgba([40, 100, 200, 255]))
            .save(&path)
            .unwrap();
        assert!(cache.get(Some(&path)).is_none());
        wait_for_images(&mut cache, &ctx);
        assert!(cache.get(Some(&path)).is_some());
        assert!(!cache.failed.contains_key(&path));
    }
    #[test]
    fn corrupt_user_cover_does_not_block_store_cover() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("custom.png");
        let good = dir.path().join("store.png");
        std::fs::write(&broken, b"invalid").unwrap();
        image::RgbaImage::from_pixel(24, 36, image::Rgba([40, 200, 100, 255]))
            .save(&good)
            .unwrap();
        let ctx = egui::Context::default();
        let mut cache = TextureCache::new(ctx.clone());
        assert!(cache.get_any([Some(&broken), Some(&good)]).is_none());
        wait_for_images(&mut cache, &ctx);
        assert_eq!(cache.get_any([Some(&broken), Some(&good)]).unwrap().2, 1);
        cache.retry_failed();
        assert!(cache.failed.is_empty());
    }
    #[test]
    fn invalid_image_does_not_decode() {
        assert!(decode_bytes(b"not an image").is_err());
    }
    #[test]
    fn normalized_matching() {
        assert_eq!(
            normalized_title("Hollow Knight™"),
            normalized_title("Hollow Knight")
        );
        assert_ne!(normalized_title("Game II"), normalized_title("Game III"));
    }
    #[test]
    fn http_errors_are_explained_without_exposing_the_key() {
        use std::{io::Write, net::TcpListener};
        for (status, expected) in [
            (401, "API-ключ не принят"),
            (403, "API-ключ не принят"),
            (429, "слишком много запросов"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status} Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                )
                .unwrap();
            });
            let service = SteamGrid {
                client: reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(std::time::Duration::from_secs(2))
                    .build()
                    .unwrap(),
                key: "fake-test-key".into(),
            };
            let error = service
                .get_url::<Vec<ArtGame>>(&format!("http://{address}"))
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected));
            assert!(!error.contains("fake-test-key"));
            server.join().unwrap();
        }
    }
    #[test]
    fn unavailable_network_returns_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let service = SteamGrid {
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_millis(200))
                .build()
                .unwrap(),
            key: "fake-test-key".into(),
        };
        assert!(
            service
                .get_url::<Vec<ArtGame>>(&format!("http://{address}"))
                .is_err()
        );
    }
}
