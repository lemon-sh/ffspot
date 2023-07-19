use color_eyre::{eyre::eyre, Result};
use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::{self, ErrorKind, Read, Write},
    path::PathBuf,
};

#[derive(Deserialize)]
pub struct Config {
    pub username: String,
    pub password: String,
    pub output: String,
    pub artists_separator: String,
    pub default_profile: String,
    #[serde(default = "default_ffpath")]
    pub ffpath: String,
    pub profiles: HashMap<String, EncodingProfile>,
}

#[derive(Deserialize)]
pub struct EncodingProfile {
    pub quality: u16,
    pub cover_art: bool,
    pub extension: String,
    pub args: Vec<String>,
}

fn default_ffpath() -> String {
    "ffmpeg".into()
}

pub enum LoadResult {
    Opened(Config),
    Created(String),
}

pub fn load() -> Result<LoadResult> {
    let path = get_configpath()?;
    match File::open(&path) {
        Ok(mut file) => {
            let mut config_str = String::new();
            file.read_to_string(&mut config_str)?;
            Ok(LoadResult::Opened(toml::from_str(&config_str)?))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let mut file = File::create(&path)?;
            file.write_all(include_bytes!("config.toml"))?;
            Ok(LoadResult::Created(path.to_string_lossy().into_owned()))
        }
        Err(err) => Err(eyre!(err)),
    }
}

fn get_configpath() -> io::Result<PathBuf> {
    if let Ok(path) = env::var("FFSPOT_CONFIG") {
        return Ok(path.into());
    }

    let configdir = if let Some(dir) = dirs::config_dir() {
        dir
    } else {
        env::current_dir()?
    }
    .join("ffspot");

    let configpath = configdir.join("config.toml");
    if !configdir.is_dir() {
        fs::create_dir_all(configdir)?;
    }

    Ok(configpath)
}
