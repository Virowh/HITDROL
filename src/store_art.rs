//! Public store artwork resolved by exact store identity; no account or personal key.
use crate::{
    artwork::decode_bytes,
    model::{Game, Source, stable_hash},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default, Debug)]
pub struct ArtPaths {
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
}
#[derive(Default, Debug)]
struct ArtUrls {
    cover: Vec<String>,
    hero: Vec<String>,
}

pub fn usable(path: Option<&PathBuf>) -> bool {
    path.is_some_and(|p| {
        std::fs::metadata(p).is_ok_and(|m| m.len() <= 24 * 1024 * 1024)
            && image::ImageReader::open(p)
                .ok()
                .and_then(|r| r.with_guessed_format().ok())
                .and_then(|r| r.into_dimensions().ok())
                .is_some_and(|(w, h)| w > 0 && h > 0 && w <= 8192 && h <= 8192)
    })
}
pub fn needs_cover(game: &Game) -> bool {
    !usable(game.user_cover.as_ref()) && !usable(game.cover.as_ref())
}
pub fn needs_hero(game: &Game) -> bool {
    !usable(game.user_hero.as_ref()) && !usable(game.hero.as_ref())
}

fn trusted(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| {
            [
                "steamstatic.com",
                "steamcdn-a.akamaihd.net",
                "epicgames.com",
                "unrealengine.com",
                "gog-statics.com",
                "images.gog.com",
            ]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
                || [
                    "api.steampowered.com",
                    "store.steampowered.com",
                    "gamesdb.gog.com",
                    "api.gog.com",
                ]
                .contains(&host)
        })
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
}

pub struct OfficialArt {
    client: reqwest::blocking::Client,
}
impl OfficialArt {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .user_agent(concat!("HITDROLLauncher/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 4 || !trusted(attempt.url()) {
                        attempt.error("Неподдерживаемое перенаправление магазина")
                    } else {
                        attempt.follow()
                    }
                }))
                .build()?,
        })
    }
    fn json(&self, url: reqwest::Url) -> Result<Value> {
        ensure!(trusted(&url), "Неподдерживаемый адрес магазина");
        let response = self.client.get(url).send()?.error_for_status()?;
        let mut bytes = vec![];
        response.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 4 * 1024 * 1024,
            "Ответ магазина слишком большой"
        );
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub fn download(&self, url: &str, cache: &Path) -> Result<PathBuf> {
        let url = reqwest::Url::parse(url)?;
        ensure!(trusted(&url), "Неподдерживаемый адрес обложки магазина");
        let path = cache.join(format!("store-{}.png", stable_hash(url.as_str())));
        if usable(Some(&path)) && decode_bytes(&std::fs::read(&path)?).is_ok() {
            return Ok(path);
        }
        let response = self.client.get(url).send()?.error_for_status()?;
        ensure!(
            response.content_length().unwrap_or(0) <= 24 * 1024 * 1024,
            "Изображение слишком большое"
        );
        let mut bytes = vec![];
        response
            .take(24 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let image = decode_bytes(&bytes)?;
        std::fs::create_dir_all(cache)?;
        let temporary = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            crate::model::timestamp()
        ));
        image.save_with_format(&temporary, image::ImageFormat::Png)?;
        std::fs::rename(temporary, &path)?;
        Ok(path)
    }
    fn first(&self, urls: &[String], cache: &Path) -> Result<Option<PathBuf>> {
        let mut error = None;
        for url in urls {
            match self.download(url, cache) {
                Ok(path) => return Ok(Some(path)),
                Err(e) => error = Some(e),
            }
        }
        match error {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
    pub fn fetch(&self, game: &Game, cache: &Path) -> Result<ArtPaths> {
        let (cover, hero) = (needs_cover(game), needs_hero(game));
        if (!cover && !hero) || game.source == Source::Manual {
            return Ok(ArtPaths::default());
        }
        let urls = match game.source {
            Source::Steam => {
                ensure!(
                    !game.source_id.is_empty()
                        && game.source_id.bytes().all(|b| b.is_ascii_digit()),
                    "Некорректный Steam ID"
                );
                let mut url = reqwest::Url::parse(
                    "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/",
                )?;
                let appid = game.source_id.parse::<u64>()?;
                let query = serde_json::json!({"ids":[{"appid":appid}],"context":{"language":"english","country_code":"US"},"data_request":{"include_assets":true}});
                url.query_pairs_mut()
                    .append_pair("input_json", &query.to_string());
                steam_urls(&self.json(url)?, &game.source_id)?
            }
            Source::Epic => ArtUrls {
                cover: game.store_cover_url.iter().cloned().collect(),
                hero: game.store_hero_url.iter().cloned().collect(),
            },
            Source::Gog => {
                ensure!(
                    !game.source_id.is_empty()
                        && game.source_id.bytes().all(|b| b.is_ascii_digit()),
                    "Некорректный GOG ID"
                );
                let url = reqwest::Url::parse(&format!(
                    "https://gamesdb.gog.com/platforms/gog/external_releases/{}",
                    game.source_id
                ))?;
                match self.json(url).and_then(|v| gog_urls(&v, &game.source_id)) {
                    Ok(urls) => urls,
                    Err(_) => {
                        let value = self.json(reqwest::Url::parse(&format!(
                            "https://api.gog.com/products/{}",
                            game.source_id
                        ))?)?;
                        ensure!(
                            id_matches(&value["id"], &game.source_id),
                            "GOG вернул другую игру"
                        );
                        let image = |field: &str| {
                            value["images"][field].as_str().map(|url| {
                                if url.starts_with("//") {
                                    format!("https:{url}")
                                } else {
                                    url.into()
                                }
                            })
                        };
                        ArtUrls {
                            cover: image("logo2x").into_iter().collect(),
                            hero: image("background").into_iter().collect(),
                        }
                    }
                }
            }
            Source::Manual => unreachable!(),
        };
        // A missing background must not throw away a successfully downloaded cover.
        let cover_result = if cover {
            self.first(&urls.cover, cache)
        } else {
            Ok(None)
        };
        let hero_result = if hero {
            self.first(&urls.hero, cache)
        } else {
            Ok(None)
        };
        match (cover_result, hero_result) {
            (Ok(cover), Ok(hero)) => Ok(ArtPaths { cover, hero }),
            (Ok(Some(cover)), _) => Ok(ArtPaths {
                cover: Some(cover),
                hero: None,
            }),
            (_, Ok(Some(hero))) => Ok(ArtPaths {
                cover: None,
                hero: Some(hero),
            }),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

fn id_matches(value: &Value, id: &str) -> bool {
    value.as_str() == Some(id) || value.as_u64().is_some_and(|n| n.to_string() == id)
}
fn steam_urls(value: &Value, id: &str) -> Result<ArtUrls> {
    let item = value["response"]["store_items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| id_matches(&i["appid"], id)))
        .context("В магазине Steam нет изображений этой игры")?;
    ensure!(
        item["success"].as_u64() == Some(1),
        "Steam не вернул данные игры"
    );
    let assets = &item["assets"];
    let format = assets["asset_url_format"]
        .as_str()
        .context("Steam не вернул адрес изображений")?;
    ensure!(
        format.starts_with(&format!("steam/apps/{id}/")) && format.contains("${FILENAME}"),
        "Некорректный адрес изображений Steam"
    );
    let urls = |fields: &[&str]| {
        fields
            .iter()
            .filter_map(|key| assets[*key].as_str())
            .filter(|p| !p.is_empty() && !p.contains("://") && !p.starts_with('/'))
            .map(|file| {
                format!(
                    "https://shared.fastly.steamstatic.com/store_item_assets/{}",
                    format.replace("${FILENAME}", file)
                )
            })
            .collect()
    };
    Ok(ArtUrls {
        cover: urls(&["library_capsule", "library_capsule_2x", "header"]),
        hero: urls(&["library_hero", "hero_capsule", "header"]),
    })
}
fn gog_urls(value: &Value, id: &str) -> Result<ArtUrls> {
    ensure!(
        value["platform_id"].as_str() == Some("gog") && id_matches(&value["external_id"], id),
        "GOG вернул другую игру"
    );
    let image = |fields: &[&str]| {
        fields
            .iter()
            .filter_map(|key| value["game"][*key]["url_format"].as_str())
            .map(|url| url.replace("{formatter}", "").replace("{ext}", "jpg"))
            .collect()
    };
    let urls = ArtUrls {
        cover: image(&["vertical_cover", "cover"]),
        hero: image(&["background", "horizontal_artwork"]),
    };
    ensure!(
        !urls.cover.is_empty() || !urls.hero.is_empty(),
        "GOG не вернул изображения"
    );
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_uses_exact_id_and_hashed_capsule_path() {
        let value = serde_json::json!({"response":{"store_items":[{"appid":42,"success":1,"assets":{"asset_url_format":"steam/apps/42/${FILENAME}?t=10","library_capsule":"abcdef/library_capsule.jpg","library_hero":"fedcba/library_hero.jpg"}}]}});
        let urls = steam_urls(&value, "42").unwrap();
        assert!(urls.cover[0].ends_with("steam/apps/42/abcdef/library_capsule.jpg?t=10"));
        assert!(steam_urls(&value, "43").is_err());
    }
    #[test]
    fn gog_vertical_cover_keeps_namespace_and_exact_identity() {
        let value = serde_json::json!({"platform_id":"gog","external_id":"123","game":{"vertical_cover":{"url_format":"https://images.gog.com/hash{formatter}.{ext}?namespace=gamesdb"}}});
        assert_eq!(
            gog_urls(&value, "123").unwrap().cover,
            vec!["https://images.gog.com/hash.jpg?namespace=gamesdb"]
        );
        assert!(gog_urls(&value, "321").is_err());
    }
    #[test]
    fn image_urls_are_limited_to_store_hosts() {
        for url in [
            "http://cdn1.epicgames.com/a.jpg",
            "https://cdn1.epicgames.com.attacker.invalid/a.jpg",
            "https://localhost/a.jpg",
            "https://user@images.gog.com/a.jpg",
        ] {
            assert!(!trusted(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "https://cdn1.epicgames.com/a",
            "https://shared.fastly.steamstatic.com/a.jpg",
            "https://images.gog.com/a.jpg?namespace=gamesdb",
        ] {
            assert!(trusted(&reqwest::Url::parse(url).unwrap()));
        }
    }
}
