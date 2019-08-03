use std::io;
use std::error::Error;
use std::collections::{HashMap, BTreeMap};

use image::{DynamicImage, GenericImageView};
use serde::{Serialize, Deserialize};
use scraper::{Html, Selector};
use reqwest::Client;
use reqwest::header::HeaderValue;
use rayon::prelude::*;
use std::path::Path;
use crate::util::make_progress_bar;

type EmoteIndex = HashMap<String, EmoteInfo>;

#[derive(Debug, Serialize, Deserialize)]
pub struct EmoteInfo {
    #[serde(rename(serialize = "from"))]
    pub provider: String,
    #[serde(rename(serialize = "type"))]
    pub img_type: String,
    pub urls: Vec<String>,
    #[serde(rename(serialize = "color"))]
    pub average_color: (u8, u8, u8),
}

impl EmoteInfo {

    fn new(
        provider_name: String, img_type: String, urls: Vec<String>,
        image: DynamicImage
    ) -> EmoteInfo {
        let [r, g, b, _] = image.thumbnail_exact(1, 1).get_pixel(0, 0).0;
        EmoteInfo {
            provider: provider_name, img_type, urls, average_color: (r, g, b)
        }
    }

}

pub trait EmoteProvider {
    fn name(&self) -> &str;
    fn fetch(&self, client: &Client, channel: Option<String>) -> Result<EmoteIndex, Box<dyn Error>>;
}

pub struct TwitchMetrics {
    client_id: String
}

impl TwitchMetrics {

    pub fn new(client_id: String) -> TwitchMetrics {
        TwitchMetrics { client_id }
    }

    fn get_user_id(&self, client: &Client, channel: &String) -> reqwest::Result<Option<String>> {
        #[derive(Deserialize)]
        struct UserId { id: String };

        #[derive(Deserialize)]
        struct UsersResponse { data: Vec<UserId> };

        const BASE_URL: &str = "https://api.twitch.tv/helix/users";
        let response: UsersResponse = client.get(BASE_URL)
            .header("Client-ID", HeaderValue::from_str(&self.client_id).expect("Invalid client ID"))
            .query(&[("login", channel.as_str())])
            .send()?
            .json()?;

        Ok(response.data.first().map(|x| x.id.clone()))
    }

}

impl EmoteProvider for TwitchMetrics {
    fn name(&self) -> &str {
        "twitchmetrics"
    }

    fn fetch(&self, client: &Client, channel: Option<String>) -> Result<EmoteIndex, Box<dyn Error>> {
        // TODO probably we can replace this heavy shit-scraping code by lightweight API call
        // the main reason to implement it like this is that *we can easily access old emotes*
        // which is great for chatan-rs in particular (because we analyze historical data)
        const BASE_URL: &str = "https://www.twitchmetrics.net";

        let url = match channel {
            None => format!("{}/emotes", BASE_URL),
            Some(channel) => {
                let user_id = self.get_user_id(client, &channel)?
                    // TODO proper error handling
                    .expect("No such channel found");
                format!("{}/c/{}-{}/emotes", BASE_URL, user_id, channel)
            }
        };
        let emote_box_selector = Selector::parse(".py-4").unwrap();
        let emote_name_selector = Selector::parse("samp").unwrap();
        let emote_link_selector = Selector::parse(".img-fluid").unwrap();

        let emote_page = Html::parse_document(&client.get(&url).send()?.text()?);

        // parse emotes in two steps. This way we can make use of parallel execution of heavy
        // (average color calculation) tasks.
        // 1) parse all emote names / urls from page
        let name_url_vec: Vec<(String, String)> = emote_page.select(&emote_box_selector)
            .filter_map(|el| {
                let emote_name = el.select(&emote_name_selector)
                    .collect::<Vec<_>>()
                    .first()?.clone()
                    .text().collect::<Vec<_>>().join("");
                let emote_link = el.select(&emote_link_selector)
                    .collect::<Vec<_>>()
                    .first()?.clone()
                    .value().attr("src")?.to_string();
                let (url, _) = emote_link.split_at(emote_link.rfind('/')?);
                Some((emote_name, url.to_string()))
            }).collect();

        // 2) create final EmoteInfo objects, possibly in parallel
        let result = name_url_vec
            .par_iter()
            .filter_map(|(name, url)| {
                let urls = vec![
                    format!("{}/1.0", url),
                    format!("{}/2.0", url),
                    format!("{}/3.0", url),
                ];

                let min_image = download_image(client, urls.first().unwrap())?;

                Some((name.to_owned(),
                      EmoteInfo::new("twitch".to_string(), "png".to_string(), urls, min_image)))
            })
            .collect::<HashMap<_, _>>();

        Ok(result)
    }

}

pub struct BetterTTV;

impl BetterTTV {
    pub fn new() -> BetterTTV {
        BetterTTV { }
    }
}

impl EmoteProvider for BetterTTV {
    fn name(&self) -> &str {
        "bttv"
    }

    fn fetch(&self, client: &Client, channel: Option<String>) -> Result<EmoteIndex, Box<dyn Error>> {
        const BASE_URL: &str = "https://api.betterttv.net/2/emotes";
        const BASE_CHANNEL_URL: &str = "https://api.betterttv.net/2/channels";

        #[derive(Deserialize)]
        struct BTTVEmote {
            id: String,
            code: String,
            #[serde(rename = "imageType")]
            image_type: String,
        };

        #[derive(Deserialize)]
        struct BTTVApiResponse {
            #[serde(rename = "urlTemplate")]
            url_template: String,
            emotes: Vec<BTTVEmote>,
        };

        let url = match channel {
            Some(channel) => format!("{}/{}", BASE_CHANNEL_URL, &channel),
            None => BASE_URL.to_string()
        };

        let emotes: BTTVApiResponse = client.get(&url).send()?.json()?;
        // this normalizes the template
        let url_template = format!("https:{}", rt_format!(&emotes.url_template).unwrap());

        let result = emotes.emotes
            .par_iter()
            .filter_map(|emote| {
                let urls = vec![
                    rt_format!(url_template, id = emote.id, image = "1x").expect("Cannot format BTTV template string"),
                    rt_format!(url_template, id = emote.id, image = "2x").expect("Cannot format BTTV template string"),
                    rt_format!(url_template, id = emote.id, image = "4x").expect("Cannot format BTTV template string"),
                ];

                let min_image = download_image(client, urls.first().unwrap())?;

                Some((emote.code.clone(),
                      EmoteInfo::new("bttv".to_string(), emote.image_type.clone(), urls, min_image)))

            })
            .collect::<HashMap<_, _>>();

        Ok(result)
    }
}

pub struct FrankerFaceZ;

impl FrankerFaceZ {
    pub fn new() -> FrankerFaceZ {
        FrankerFaceZ {}
    }
}

impl EmoteProvider for FrankerFaceZ {
    fn name(&self) -> &str {
        "ffz"
    }

    fn fetch(&self, client: &Client, channel: Option<String>) -> Result<EmoteIndex, Box<dyn Error>> {
        const BASE_URL: &str = "https://api.frankerfacez.com/v1/set/global";
        const BASE_CHANNEL_URL: &str = "https://api.frankerfacez.com/v1/room";

        #[derive(Deserialize)]
        struct FFZEmote {
            name: String,
            urls: BTreeMap<i32, String>,
        };

        #[derive(Deserialize)]
        struct FFZEmoteSet {
            emoticons: Vec<FFZEmote>,
        }

        #[derive(Deserialize)]
        struct FFZApiResponse {
            sets: HashMap<String, FFZEmoteSet>,
        };

        let url = match channel {
            Some(channel) => format!("{}/{}", BASE_CHANNEL_URL, &channel),
            None => BASE_URL.to_string()
        };

        let emotes: FFZApiResponse = client.get(&url).send()?.json()?;

        let result = emotes.sets
            .iter()
            .flat_map(|(_, set)| set.emoticons.iter())
            .par_bridge()
            .filter_map(|emote: &FFZEmote| {
                let urls = emote.urls.iter()
                    .map(|(_, url)| format!("https:{}", url))
                    .collect::<Vec<String>>();

                let min_image = download_image(client, urls.first()?)?;

                Some((emote.name.clone(),
                      EmoteInfo::new("ffz".to_string(), "png".to_string(), urls, min_image)))

            })
            .collect::<HashMap<_, _>>();

        Ok(result)
    }
}

fn download_image(client: &Client, url: &String) -> Option<DynamicImage> {
    let mut buf = Vec::<u8>::new();
    client.get(url).send().ok()?.copy_to(&mut buf).ok()?;
    image::load_from_memory(buf.as_slice()).ok()
}

pub fn merge_indexes(indexes: Vec<EmoteIndex>) -> EmoteIndex {
    let mut result = HashMap::new();
    indexes.into_iter().for_each(|index| result.extend(index));
    result
}

pub fn save_index(path: &Path, index: &EmoteIndex) -> io::Result<()> {
    let file_writer = io::BufWriter::new(std::fs::File::create(path)?);
    let result = serde_json::to_writer(file_writer, index)?;
    Ok(result)
}

pub fn load_index(path: &Path) -> io::Result<EmoteIndex> {
    let file_reader = io::BufReader::new(std::fs::File::open(path)?);
    let result = serde_json::from_reader(file_reader)?;
    Ok(result)
}

pub fn build_index(client: &Client, channels: Vec<String>, providers: Vec<Box<dyn EmoteProvider>>) -> EmoteIndex {
    let bar = make_progress_bar((1 + channels.len()) * providers.len());

    let mut emotes = Vec::new();

    emotes.extend(providers.iter().map(|provider| {
        bar.set_message(format!("global @ {}", provider.name()).as_str());
        let r = provider.fetch(&client, None).expect("Cannot load global emotes");
        bar.inc(1);
        r
    }));

    for provider in providers {
        emotes.extend(
            channels.iter().map(|channel| {
                bar.set_message(format!("{} @ {}", &channel, provider.name()).as_str());
                let r = provider.fetch(&client, Some(channel.clone()))
                    .expect("Cannot load emotes for channel");
                bar.inc(1);
                r
            })
        );
    }

    bar.finish();

    merge_indexes(emotes)
}

pub fn update_index_in_path(client: &Client, path: &Path, channels: Vec<String>, providers: Vec<Box<dyn EmoteProvider>>)
    -> io::Result<EmoteIndex> {
    let old = load_index(path).unwrap_or_else(|_| EmoteIndex::new());

    let index = merge_indexes(
        vec![old, build_index(client, channels, providers)]
    );

    save_index(&path, &index)?;

    Ok(index)
}
