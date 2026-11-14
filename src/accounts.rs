//! Account libraries, separate from the local installation scan. No passwords or
//! browser cookies are read. Only refresh tokens are persisted, in Windows credentials.
use crate::{model::*, platform};
use anyhow::{Context, Result, ensure};
use reqwest::{
    Url,
    blocking::{Client, RequestBuilder},
};
use serde_json::Value;
use std::{
    collections::HashSet,
    io::Read,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

// Public desktop-client OAuth identifiers (protocol constants, not user credentials).
const EPIC_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const EPIC_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
const GOG_ID: &str = "46899977096215655";
const GOG_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const GOG_REDIRECT: &str = "https://embed.gog.com/on_login_success?origin=client";

pub fn login_url(source: Source) -> Result<Url> {
    match source {
        Source::Epic => {
            let redirect = format!(
                "https://www.epicgames.com/id/api/redirect?clientId={EPIC_ID}&responseType=code"
            );
            Ok(Url::parse_with_params(
                "https://www.epicgames.com/id/login",
                &[("redirectUrl", redirect.as_str())],
            )?)
        }
        Source::Gog => Ok(Url::parse_with_params(
            "https://auth.gog.com/auth",
            &[
                ("client_id", GOG_ID),
                ("redirect_uri", GOG_REDIRECT),
                ("response_type", "code"),
                ("layout", "client2"),
            ],
        )?),
        _ => anyhow::bail!("Войдите в установленный клиент Steam"),
    }
}
pub fn authorization_code(source: Source, input: &str) -> Result<String> {
    let input = input.trim();
    ensure!(input.len() <= 8192, "Слишком длинный код входа");
    let code = if input.starts_with('{') {
        ensure!(
            source == Source::Epic,
            "Для GOG вставьте адрес страницы после входа"
        );
        serde_json::from_str::<Value>(input)
            .ok()
            .and_then(|v| v["authorizationCode"].as_str().map(str::to_owned))
    } else if input.starts_with("https://") {
        let url = Url::parse(input).context("Неверный адрес после входа")?;
        let host = url.host_str().unwrap_or_default();
        ensure!(
            match source {
                Source::Epic => host == "www.epicgames.com" || host == "epicgames.com",
                Source::Gog => host == "embed.gog.com" || host == "auth.gog.com",
                _ => false,
            },
            "Вставьте адрес страницы магазина после входа"
        );
        url.query_pairs()
            .find(|(k, _)| k == "code" || k == "authorizationCode")
            .map(|(_, v)| v.into_owned())
    } else {
        Some(input.to_owned())
    };
    let code =
        code.context("В адресе нет кода входа. Завершите вход в браузере и скопируйте результат.")?;
    ensure!(
        (8..=2048).contains(&code.len())
            && code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "Нужен одноразовый код, а не пароль или адрес страницы входа"
    );
    Ok(code)
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent("HITDROL/0.3 account-library")
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn json(request: RequestBuilder) -> Result<Value> {
    // Never expose reqwest errors or response bodies: OAuth URLs contain secrets.
    let response = request
        .send()
        .map_err(|_| anyhow::anyhow!("Нет связи с магазином. Проверьте сеть и повторите."))?;
    let status = response.status();
    ensure!(
        status.is_success(),
        "Магазин отклонил запрос (HTTP {}). При ошибке входа подключите аккаунт заново.",
        status.as_u16()
    );
    ensure!(
        response.content_length().unwrap_or(0) <= 16 * 1024 * 1024,
        "Слишком большой ответ магазина"
    );
    let mut body = Vec::new();
    response
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| anyhow::anyhow!("Ответ магазина не удалось прочитать"))?;
    ensure!(
        body.len() <= 16 * 1024 * 1024,
        "Слишком большой ответ магазина"
    );
    serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("Магазин вернул неизвестный формат данных"))
}
fn authenticate(client: &Client, source: Source, code: Option<&str>) -> Result<String> {
    let refresh;
    let (grant, key, value) = if let Some(code) = code {
        ("authorization_code", "code", code)
    } else {
        refresh = platform::library_token(source)?.context("Подключите аккаунт в настройках")?;
        ("refresh_token", "refresh_token", refresh.as_str())
    };
    let data=match source {
        Source::Epic => json(client.post("https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token")
            .basic_auth(EPIC_ID,Some(EPIC_SECRET)).form(&[("grant_type",grant),(key,value),("token_type","eg1")]))?,
        Source::Gog => json(client.get("https://auth.gog.com/token").query(&[("client_id",GOG_ID),("client_secret",GOG_SECRET),
            ("grant_type",grant),(key,value),("redirect_uri",GOG_REDIRECT)]))?,
        _=>anyhow::bail!("Источник не поддерживает подключение"),
    };
    let access = data["access_token"]
        .as_str()
        .context("Магазин не подтвердил вход")?;
    let refresh = data["refresh_token"]
        .as_str()
        .context("Магазин не выдал токен обновления")?;
    platform::save_library_token(source, refresh)?;
    Ok(access.into())
}
fn owned_game(
    source: Source,
    id: String,
    title: String,
    target: LaunchTarget,
    install: LaunchTarget,
) -> Game {
    let mut game = Game::new(source, id, title, target);
    game.owned = true;
    game.installed = false;
    game.available = false;
    game.install_target = Some(install);
    game
}
fn epic_game(asset: &Value, info: &Value) -> Option<Game> {
    let app = asset["appName"].as_str()?;
    let ns = asset["namespace"].as_str()?;
    let id = asset["catalogItemId"].as_str()?;
    if !info["mainGameItem"].is_null() {
        return None;
    } // DLC is installed with its base game.
    let path = format!("{ns}:{id}:{app}");
    let encoded: String = path
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let mut game = owned_game(
        Source::Epic,
        app.into(),
        info["title"].as_str().unwrap_or(app).into(),
        LaunchTarget::uri(format!(
            "com.epicgames.launcher://apps/{encoded}?action=launch&silent=true"
        )),
        LaunchTarget::uri(format!(
            "com.epicgames.launcher://apps/{encoded}?action=install&silent=false"
        )),
    );
    let images = info["keyImages"].as_array().cloned().unwrap_or_default();
    let image = |types: &[&str]| {
        types.iter().find_map(|t| {
            images
                .iter()
                .find(|i| i["type"].as_str() == Some(t))
                .and_then(|i| i["url"].as_str())
                .map(str::to_owned)
        })
    };
    game.store_cover_url = image(&[
        "DieselGameBoxTall",
        "OfferImageTall",
        "Thumbnail",
        "DieselGameBox",
    ]);
    game.store_hero_url = image(&["DieselGameBox", "OfferImageWide", "Featured"]);
    Some(game)
}
fn gog_game(id: &str, info: &Value) -> Game {
    let mut game = owned_game(
        Source::Gog,
        id.into(),
        info["title"].as_str().unwrap_or("Игра GOG").into(),
        LaunchTarget::uri(format!("goggalaxy://openGameView/{id}")),
        LaunchTarget::uri(format!("goggalaxy://openGameView/{id}")),
    );
    game.store_cover_url = info["images"]["logo2x"].as_str().map(|s| {
        if s.starts_with("//") {
            format!("https:{s}")
        } else {
            s.into()
        }
    });
    game
}
fn gog_ids(value: &Value) -> Result<Vec<String>> {
    let list = value["owned"]
        .as_array()
        .context("GOG не вернул список принадлежащих аккаунту игр")?;
    ensure!(list.len() <= 20000, "Слишком большая библиотека GOG");
    let mut ids = HashSet::new();
    for id in list {
        let id = id
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| id.as_str().map(str::to_owned))
            .context("Неверный идентификатор GOG")?;
        ensure!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
            "Неверный идентификатор GOG"
        );
        ids.insert(id);
    }
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort();
    Ok(ids)
}
/// Metadata downloads are bounded to four workers; failure never fabricates ownership.
pub fn sync(
    source: Source,
    code: Option<&str>,
    cached: &[Game],
    progress: impl Fn(usize, usize) + Sync,
) -> Result<Vec<Game>> {
    let client = client()?;
    let access = authenticate(&client, source, code)?;
    let assets = match source {
        Source::Epic => {
            let data=json(client.get("https://launcher-public-service-prod06.ol.epicgames.com/launcher/api/public/assets/Windows").query(&[("label","Live")]).bearer_auth(&access))?;
            data.as_array()
                .context("Epic не вернул библиотеку Windows")?
                .clone()
        }
        Source::Gog => gog_ids(&json(
            client
                .get("https://embed.gog.com/user/data/games")
                .bearer_auth(&access),
        )?)?
        .into_iter()
        .map(Value::String)
        .collect(),
        _ => anyhow::bail!("Источник не поддерживается"),
    };
    ensure!(assets.len() <= 20000, "Слишком большая библиотека аккаунта");
    if source == Source::Epic {
        ensure!(
            assets
                .iter()
                .all(|a| ["appName", "namespace", "catalogItemId"]
                    .iter()
                    .all(|key| a[*key].as_str().is_some_and(|s| !s.is_empty()))),
            "Epic вернул неполные идентификаторы игр. Сохранённая библиотека оставлена без изменений."
        );
    }
    let index = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let metadata_failures = AtomicUsize::new(0);
    progress(0, assets.len());
    let mut games = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..4 {
            workers.push(scope.spawn(|| {
                let mut games = Vec::new();
                loop {
                    let i = index.fetch_add(1, Ordering::Relaxed);
                    let Some(asset) = assets.get(i) else { break };
                    let id = if source == Source::Epic {
                        asset["appName"].as_str()
                    } else {
                        asset.as_str()
                    };
                    let Some(id) = id else { continue };
                    let old = cached
                        .iter()
                        .find(|g| g.source == source && g.source_id == id);
                    // Already cached official metadata needs no per-item network call on refresh.
                    let game = if let Some(old) = old.filter(|g| {
                        g.title != id && g.title != "Игра GOG" && (g.owned || g.installed)
                    }) {
                        let mut g = (*old).clone();
                        g.owned = true;
                        Some(g)
                    } else if source == Source::Epic {
                        let ns = asset["namespace"].as_str().unwrap_or_default();
                        let item = asset["catalogItemId"].as_str().unwrap_or_default();
                        let mut url =
                            Url::parse("https://catalog-public-service-prod06.ol.epicgames.com")
                                .unwrap();
                        url.path_segments_mut().unwrap().extend([
                            "catalog",
                            "api",
                            "shared",
                            "namespace",
                            ns,
                            "bulk",
                            "items",
                        ]);
                        let request = client.get(url).bearer_auth(&access).query(&[
                            ("id", item),
                            ("includeDLCDetails", "true"),
                            ("includeMainGameDetails", "true"),
                            ("country", "US"),
                            ("locale", "en"),
                        ]);
                        let info = if metadata_failures.load(Ordering::Relaxed) < 4 {
                            json(request).unwrap_or_else(|_| {
                                metadata_failures.fetch_add(1, Ordering::Relaxed);
                                Value::Null
                            })
                        } else {
                            Value::Null
                        };
                        epic_game(asset, &info[item])
                    } else {
                        let info = if metadata_failures.load(Ordering::Relaxed) < 4 {
                            json(client.get(format!("https://api.gog.com/products/{id}")))
                                .unwrap_or_else(|_| {
                                    metadata_failures.fetch_add(1, Ordering::Relaxed);
                                    Value::Null
                                })
                        } else {
                            Value::Null
                        };
                        Some(gog_game(id, &info))
                    };
                    if let Some(game) = game {
                        games.push(game);
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(n, assets.len());
                }
                games
            }));
        }
        let mut games = Vec::new();
        for worker in workers {
            games.extend(
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("Сбой чтения библиотеки магазина"))?,
            );
        }
        Ok::<_, anyhow::Error>(games)
    })?;
    games.sort_by(|a, b| a.id.cmp(&b.id));
    games.dedup_by(|a, b| a.id == b.id);
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_accept_official_results_and_reject_passwords_and_other_sites() {
        assert_eq!(
            authorization_code(Source::Epic, r#"{"authorizationCode":"12345678abc"}"#).unwrap(),
            "12345678abc"
        );
        assert_eq!(
            authorization_code(
                Source::Gog,
                "https://embed.gog.com/on_login_success?origin=client&code=abcdef1234"
            )
            .unwrap(),
            "abcdef1234"
        );
        assert!(
            authorization_code(Source::Epic, "https://attacker.invalid/?code=123456789").is_err()
        );
        assert!(authorization_code(Source::Gog, "password with spaces").is_err());
        assert_eq!(
            login_url(Source::Gog).unwrap().host_str(),
            Some("auth.gog.com")
        );
    }
    #[test]
    fn exact_owned_ids_and_install_targets() {
        assert_eq!(
            gog_ids(&serde_json::json!({"owned":[42,"42",7]})).unwrap(),
            ["42", "7"]
        );
        assert!(gog_ids(&serde_json::json!({"products":[42]})).is_err());
        let asset = serde_json::json!({"appName":"app","namespace":"ns","catalogItemId":"item"});
        let game = epic_game(&asset, &serde_json::json!({"title":"Game"})).unwrap();
        assert!(!game.installed && game.owned);
        assert_eq!(game.source_id, "app");
        assert!(
            game.install_target
                .unwrap()
                .path
                .ends_with("ns%3Aitem%3Aapp?action=install&silent=false")
        );
        assert!(epic_game(&asset, &serde_json::json!({"mainGameItem":{"id":"base"}})).is_none());
    }
}
