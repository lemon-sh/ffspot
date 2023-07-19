use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsString,
    io::{self, Seek},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

use async_tempfile::TempFile;
use color_eyre::{eyre::eyre, Result};
use indicatif::{ProgressBar, ProgressStyle};
use librespot::{
    audio::{AudioDecrypt, AudioFile},
    core::{session::Session, spotify_id::FileId},
    metadata::audio::AudioFileFormat,
};
use tokio::{task, io::AsyncWriteExt};

use crate::{
    config::Config,
    resolve,
    template::{Template, TemplateFields},
};

fn select_file(
    files: &HashMap<AudioFileFormat, FileId>,
    allowed_formats: &[AudioFileFormat],
) -> Option<FileId> {
    for allowed_format in allowed_formats {
        if let Some(file) = files.get(allowed_format) {
            return Some(*file);
        }
    }
    None
}

pub async fn download(
    resource_type: &str,
    resource_id: &str,
    path_template: Template,
    session: Session,
    mut cfg: Config,
    skip_existing: bool,
    encoding_profile: Option<&str>,
) -> Result<()> {
    let profile_name = encoding_profile.unwrap_or(&cfg.default_profile);
    let Some(profile) = cfg.profiles.remove(profile_name) else {
        return Err(eyre!("Encoding profile {profile_name} not found"))
    };

    let allowed_formats: &[AudioFileFormat] = match profile.quality {
        320 => &[
            AudioFileFormat::OGG_VORBIS_320,
            AudioFileFormat::OGG_VORBIS_160,
            AudioFileFormat::OGG_VORBIS_96,
        ],
        160 => &[
            AudioFileFormat::OGG_VORBIS_160,
            AudioFileFormat::OGG_VORBIS_96,
        ],
        96 => &[AudioFileFormat::OGG_VORBIS_96],
        e => return Err(eyre!("Invalid quality '{e}'")),
    };

    let mut profile_ffargs = Vec::with_capacity(profile.args.len());
    for arg in &profile.args {
        profile_ffargs.push(Template::compile(arg)?);
    }

    let style_int = ProgressStyle::with_template(
        "{spinner:.green} [{bar:40.blue}] {pos}/{len} {wide_msg:.green}",
    )
    .unwrap()
    .progress_chars("-> ");

    let style_data = ProgressStyle::with_template(
        "{spinner:.green} [{bar:40.blue}] {bytes}/{total_bytes} {bytes_per_sec} {wide_msg:.green}",
    )
    .unwrap()
    .progress_chars("-> ");

    let metadata_pb = ProgressBar::new(0);
    metadata_pb.set_style(style_int.clone());
    metadata_pb.set_message("Resolving track metadata");

    let tracks = resolve::resolve_tracks(resource_type, resource_id, &session, metadata_pb).await?;

    let track_count = tracks.len();
    let seq_digits = track_count.to_string().len();

    let ffpath = Arc::new(OsString::from(cfg.ffpath));

    for (mut seq, track) in tracks.into_iter().enumerate() {
        seq += 1;

        let mut author = String::new();
        let last_n = track.artists.len() - 1;
        for (n, artist) in track.artists.0.iter().enumerate() {
            author.push_str(&artist.name);
            if n != last_n {
                author.push_str(&cfg.artists_separator);
            }
        }

        let template_fields = TemplateFields {
            author: &author,
            track: &track.name,
            album: &track.album.name,
            extension: &profile.extension,
            seq,
            seq_digits,
        };

        let path_string = path_template.resolve(&template_fields)?;
        let path = Path::new(&path_string);

        if skip_existing && path.exists() {
            continue;
        }

        let filename = path.file_name().map_or_else(|| path.to_string_lossy().to_string(), |v| v.to_string_lossy().to_string());

        let display_id = track.id.to_base62()?;

        let file = select_file(&track.files, allowed_formats)
            .ok_or_else(|| eyre!("Could not find a suitable file for track {display_id:?}"))?;

        let key = session.audio_key().request(track.id, file).await?;

        let stream = AudioFile::open(&session, file, 1024 * 1024 * 1024).await?;

        let controller = stream.get_stream_loader_controller()?;
        let size = controller.len();
        controller.set_stream_mode();

        let download_pb = ProgressBar::new(size as u64);
        download_pb.set_style(style_data.clone());

        let mut raw_stream = download_pb.wrap_read(AudioDecrypt::new(Some(key), stream));

        let ffpath = ffpath.clone();

        let mut ffargs: Vec<Cow<'static, str>> = vec![
            "-y".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-i".into(),
            "-".into(),
        ];

        let mut covers = track.album.covers.0;
        let mut _cover = None;

        if profile.cover_art && !covers.is_empty() {
            download_pb.set_message(format!(
                "(downloading cover art...) [{seq}/{track_count}] {filename}"
            ));
            covers.sort_by_key(|i| i.width * i.height);
            let cover_id = covers.first().unwrap().id;
            let cover_url = format!("https://i.scdn.co/image/{cover_id}");
            let mut cover_resp = reqwest::get(cover_url).await?.error_for_status()?;
            let mut cover_file = TempFile::new().await?;
            while let Some(chunk) = cover_resp.chunk().await? {
                cover_file.write_all(&chunk).await?;
            }
            ffargs.push("-i".into());
            ffargs.push(cover_file.file_path().to_string_lossy().into_owned().into());
            _cover = Some(cover_file);
        }

        for arg in &profile_ffargs {
            ffargs.push(arg.resolve(&template_fields)?.into());
        }

        ffargs.push(path_string.into());

        tracing::debug!("ffmpeg args built: {ffargs:?}");

        let task = task::spawn_blocking(move || {
            download_pb.set_message(format!("[{seq}/{track_count}] {filename}"));

            let mut ffmpeg = Command::new(&*ffpath)
                .args(ffargs.iter().map(AsRef::as_ref))
                .stderr(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stdin(Stdio::piped())
                .spawn()?;
            let mut stdin = ffmpeg.stdin.take().unwrap();

            raw_stream.seek(io::SeekFrom::Start(167))?;
            io::copy(&mut raw_stream, &mut stdin)?;

            drop(stdin);

            let status = ffmpeg.wait()?;
            if status.success() {
                Ok(())
            } else if let Some(code) = status.code() {
                Err(eyre!("ffmpeg exited with a non-zero exit code: {code}"))
            } else {
                Err(eyre!("ffmpeg was terminated by a signal"))
            }
        });

        task.await??;
    }

    Ok(())
}
