use reqwest;
use rss::{self, Channel, Item};
use std::fs::{DirBuilder, File};
use std::io::{self, Read, Write};
use utils::*;
use serde_json;
use std::collections::BTreeSet;

#[derive(Serialize, Deserialize, Clone)]
pub struct Subscription {
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct State(Vec<Subscription>);

impl State {
    pub fn new() -> State {
        let mut path = get_podcast_dir();
        path.push(".subscriptions");
        if path.exists() {
            let mut s = String::new();
            File::open(&path).unwrap().read_to_string(&mut s).unwrap();
            serde_json::from_str(&s).unwrap()
        } else {
            State(Vec::new())
        }
    }

    pub fn subscribe(&mut self, url: &str) {
        let mut set = BTreeSet::new();
        for sub in self.subscriptions() {
            set.insert(sub.url);
        }
        if !set.contains(url) {
            let channel = Channel::from_url(url).unwrap();
            self.0.push(Subscription {
                name: String::from(channel.title()),
                url: String::from(url),
            });
        }
        match self.save() {
            Err(err) => println!("{}", err),
            _ => (),
        }
    }

    pub fn subscriptions(&self) -> Vec<Subscription> {
        self.0.clone()
    }

    pub fn save(&self) -> Result<(), io::Error> {
        let mut path = get_podcast_dir();
        path.push(".subscriptions");
        let serialized = serde_json::to_string(self)?;
        let mut file = File::create(&path)?;
        file.write_all(serialized.as_bytes())?;
        Ok(())
    }
}

pub struct Podcast(Channel);

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

    pub fn list_episodes(&self) -> Vec<&str> {
        let mut result = Vec::new();

        let items = self.0.items();
        for item in items {
            match item.title() {
                Some(val) => result.push(val),
                None => (),
            }
        }
        result
    }

    pub fn download(&self) {
        let mut path = get_podcast_dir();
        path.push(self.title());

        DirBuilder::new().recursive(true).create(path).unwrap();

        let downloaded = already_downloaded(self.title());

        for ep in self.episodes() {
            if let Some(ep_title) = ep.title() {
                if !downloaded.contains(ep_title) {
                    match ep.download(self.title()) {
                        Err(err) => println!("{}", err),
                        _ => (),
                    }
                }
            }
        }
    }
}

impl Episode {
    pub fn title(&self) -> Option<&str> {
        self.0.title()
    }

    pub fn download_url(&self) -> Option<&str> {
        match self.0.enclosure() {
            Some(val) => Some(val.url()),
            None => None,
        }
    }

    fn download_extension(&self) -> Option<&str> {
        match self.0.enclosure() {
            Some(enclosure) => {
                match enclosure.mime_type() {
                    "audio/mpeg" => Some(".mp3"),
                    "audio/mp4" => Some(".m4a"),
                    "audio/ogg" => Some(".ogg"),
                    _ => None,
                }
            }
            None => None,
        }
    }

    pub fn download(&self, podcast_name: &str) -> Result<(), io::Error> {
        let mut path = get_podcast_dir();
        path.push(podcast_name);

        if let Some(url) = self.download_url() {
            if let Some(title) = self.title() {
                println!("Downloading: {}", title);
                let mut filename = String::from(title);
                filename.push_str(self.download_extension().unwrap());
                path.push(filename);

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