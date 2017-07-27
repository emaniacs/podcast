use actions::*;
use chrono::prelude::*;
use rayon::prelude::*;
use reqwest;
use rss::{self, Channel, Item};
use serde_json;
use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read, Write};
use utils::*;
use yaml_rust::YamlLoader;

pub struct Config {
    pub auto_download_limit: i64,
    pub auto_delete_limit: i64,
}

impl Config {
    pub fn new() -> Config {
        let mut path = get_podcast_dir();
        let mut download_limit = 1;
        let mut delete_limit = 0;
        path.push(".config");
        if path.exists() {
            let mut s = String::new();
            File::open(&path).unwrap().read_to_string(&mut s).unwrap();
            let config = YamlLoader::load_from_str(&s).unwrap();
            if config.len() > 0 {
                let doc = &config[0];
                if let Some(val) = doc["auto_download_limit"].as_i64() {
                    download_limit = val;
                }
                if let Some(val) = doc["auto_delete_limit"].as_i64() {
                    delete_limit = val;
                }
            }
        } else {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"auto_download_limit: 1").unwrap();
        }
        Config {
            auto_download_limit: download_limit,
            auto_delete_limit: delete_limit,
        }
    }
}


#[derive(Serialize, Deserialize, Clone)]
pub struct Subscription {
    pub title: String,
    pub url: String,
    pub num_episodes: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct State {
    pub last_run_time: DateTime<Utc>,
    pub subs: Vec<Subscription>,
}

impl State {
    pub fn new() -> Result<State, String> {
        let mut path = get_podcast_dir();
        path.push(".subscriptions");
        if path.exists() {
            let mut s = String::new();
            let mut file = match File::open(&path) {
                Ok(val) => val,
                Err(err) => return Err(format!("{}", err)),
            };
            if let Err(err) = file.read_to_string(&mut s) {
                return Err(format!("{}", err));
            };
            let mut state: State = match serde_json::from_str(&s) {
                Ok(val) => val,
                Err(err) => {
                    return Err(format!(
                        "Could not parse: {}\nReason: {}",
                        &path.to_str().unwrap(),
                        err
                    ))
                }
            };
            // Check if a day has passed (86400 seconds)
            if state
                .last_run_time
                .signed_duration_since(Utc::now())
                .num_seconds() < -86400
            {
                update_rss(&mut state);
            }
            state.last_run_time = Utc::now();
            Ok(state)
        } else {
            Ok(State {
                last_run_time: Utc::now(),
                subs: Vec::new(),
            })
        }
    }

    pub fn subscribe(&mut self, url: &str, config: &Config) {
        let mut set = BTreeSet::new();
        for sub in self.subscriptions() {
            set.insert(sub.title);
        }
        let podcast = Podcast::from(Channel::from_url(url).unwrap());
        if !set.contains(podcast.title()) {
            self.subs.push(Subscription {
                title: String::from(podcast.title()),
                url: String::from(url),
                num_episodes: podcast.episodes().len(),
            });
        }
        if let Err(err) = self.save() {
            eprintln!("{}", err);
        }
        download_rss(url, config);
    }

    pub fn subscriptions(&self) -> Vec<Subscription> {
        self.subs.clone()
    }

    pub fn save(&self) -> Result<(), io::Error> {
        let mut path = get_podcast_dir();
        path.push(".subscriptions.tmp");
        let serialized = serde_json::to_string(self)?;
        let mut file = File::create(&path)?;
        file.write_all(serialized.as_bytes())?;
        fs::rename(&path, get_sub_file())?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Podcast(Channel);

#[derive(Clone)]
pub struct Episode(Item);

impl From<Channel> for Podcast {
    fn from(channel: Channel) -> Podcast {
        Podcast(channel)
    }
}

impl From<Item> for Episode {
    fn from(item: Item) -> Episode {
        Episode(item)
    }
}

impl Podcast {
    pub fn title(&self) -> &str {
        self.0.title()
    }

    #[allow(dead_code)]
    pub fn url(&self) -> &str {
        self.0.link()
    }

    pub fn from_url(url: &str) -> Result<Podcast, rss::Error> {
        match Channel::from_url(url) {
            Ok(val) => Ok(Podcast::from(val)),
            Err(err) => Err(err),
        }
    }

    pub fn episodes(&self) -> Vec<Episode> {
        let mut result = Vec::new();

        let items = self.0.items().to_vec();
        for item in items {
            result.push(Episode::from(item));
        }
        result
    }

    pub fn download(&self) {
        let mut path = get_podcast_dir();
        path.push(self.title());

        let downloaded = already_downloaded(self.title());

        self.episodes().par_iter().for_each(
            |ref i| if let Some(ep_title) =
                i.title()
            {
                if !downloaded.contains(ep_title) {
                    if let Err(err) = i.download(self.title()) {
                        println!("{}", err);
                    }
                }
            },
        );
    }

    pub fn download_specific(&self, episode_numbers: Vec<usize>) {
        let mut path = get_podcast_dir();
        path.push(self.title());

        let downloaded = already_downloaded(self.title());
        let episodes = self.episodes();

        episode_numbers.par_iter().for_each(
            |ep_num| if let Some(ep_title) =
                episodes[episodes.len() - ep_num].title()
            {
                if !downloaded.contains(ep_title) {
                    if let Err(err) = episodes[episodes.len() - ep_num].download(self.title()) {
                        println!("{}", err);
                    }
                }
            },
        );
    }
}

impl Episode {
    pub fn title(&self) -> Option<&str> {
        self.0.title()
    }

    pub fn url(&self) -> Option<&str> {
        match self.0.enclosure() {
            Some(val) => Some(val.url()),
            None => None,
        }
    }

    pub fn extension(&self) -> Option<&str> {
        match self.0.enclosure() {
            Some(enclosure) => {
                match enclosure.mime_type() {
                    "audio/mpeg" => Some(".mp3"),
                    "audio/mp4" => Some(".m4a"),
                    "audio/ogg" => Some(".ogg"),
                    _ => find_extension(self.url().unwrap()),
                }
            }
            None => None,
        }
    }


    pub fn download(&self, podcast_name: &str) -> Result<(), io::Error> {
        let mut path = get_podcast_dir();
        path.push(podcast_name);
        DirBuilder::new().recursive(true).create(&path).unwrap();

        if let Some(url) = self.url() {
            if let Some(title) = self.title() {
                let mut filename = String::from(title);
                filename.push_str(self.extension().unwrap());
                path.push(filename);
                println!("Downloading: {}", path.to_str().unwrap());
                let mut file = File::create(&path)?;
                let mut resp = reqwest::get(url).unwrap();
                let mut content: Vec<u8> = Vec::new();
                resp.read_to_end(&mut content)?;
                file.write_all(&content)?;
                return Ok(());
            }
        }
        Ok(())
    }
}
