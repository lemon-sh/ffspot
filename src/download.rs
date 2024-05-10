use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsString,
    fs,
    io::{self, ErrorKind, Read},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

use async_tempfile::TempFile;
use color_eyre::{
    eyre::{bail, eyre, OptionExt},
    Result,
};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use librespot::{
    audio::AudioDecrypt,
    core::{cdn_url::CdnUrl, session::Session, spotify_id::FileId},
    metadata::{audio::AudioFileFormat, Track},
};
use tokio::{
    fs::{create_dir_all, OpenOptions},
    io::AsyncWriteExt,
    task,
};
use ureq::Response;

use crate::{
    cli::Args,
    config::{Config, EncodingProfile},
    resolve,
    template::{self, Template},
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
    session: Session,
    mut cfg: Config,
    cli: &Args,
) -> Result<()> {
    let path_template = Template::compile(cli.output.as_deref().unwrap_or(&cfg.output))?;
    let profile_name = cli
        .encoding_profile
        .as_deref()
        .unwrap_or(&cfg.default_profile);
    let Some(profile) = cfg.profiles.remove(profile_name) else {
        bail!("Encoding profile {profile_name:?} not found");
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

    let pbstyle_int = ProgressStyle::with_template(
        "{spinner:.green} [{bar:40.blue}] {pos}/{len} {wide_msg:.green}",
    )
    .unwrap()
    .progress_chars("-> ");

    let pbstyle_data = ProgressStyle::with_template(
        "{spinner:.green} [{bar:40.blue}] {bytes}/{total_bytes} {bytes_per_sec} {wide_msg:.green}",
    )
    .unwrap()
    .progress_chars("-> ");

    let metadata_pb = ProgressBar::new(0);
    metadata_pb.set_style(pbstyle_int.clone());
    metadata_pb.set_message("Resolving track metadata");

    let tracks = resolve::resolve_tracks(resource_type, resource_id, &session, metadata_pb).await?;

    let track_count = tracks.len();
    let seq_max_digits = track_count.to_string().len();

    let ffpath = Arc::new(OsString::from(&cfg.ffpath));

    let mut errors = Vec::new();
    let mut skipped = 0;

    for (seq, track) in tracks.into_iter().enumerate() {
        let track_id = track.id;
        let result = download_track(
            track,
            &path_template,
            &session,
            &cfg,
            cli.skip_existing,
            &profile,
            seq + 1,
            seq_max_digits,
            allowed_formats,
            pbstyle_data.clone(),
            ffpath.clone(),
            track_count,
            &profile_ffargs,
            cli.external_cover_art.as_deref(),
        )
        .await;

        match result {
            Err(e) => errors.push((e, track_id)),
            Ok(o) if !o => skipped += 1,
            Ok(_) => {}
        }
    }

    let error_count = errors.len();

    for (error, id) in errors {
        eprintln!(
            "{} {id}\n{error:?}",
            "An error has occurred while downloading track".bright_red()
        );
    }

    eprintln!(
        "{} ({skipped} {}, {} {})",
        "Done!".bright_green(),
        "skipped".bright_cyan(),
        error_count,
        "errors".bright_cyan()
    );

    Ok(())
}

async fn download_track(
    track: Track,
    path_template: &Template,
    session: &Session,
    cfg: &Config,
    skip_existing: bool,
    profile: &EncodingProfile,
    seq: usize,
    seq_max_digits: usize,
    allowed_formats: &[AudioFileFormat],
    pb_style: ProgressStyle,
    ffpath: Arc<OsString>,
    track_count: usize,
    profile_ffargs: &[Template],
    external_cover_art: Option<&str>,
) -> Result<bool> {
    let mut artists = String::new();
    let last_n = track.artists.len() - 1;
    for (n, artist) in track.artists.0.iter().enumerate() {
        artists.push_str(&artist.name);
        if n != last_n {
            artists.push_str(&cfg.artists_separator);
        }
    }

    let template_fields = template::Fields {
        artists: artists.into(),
        title: track.name.into(),
        album: track.album.name.into(),
        seq,
        seq_digits: seq_max_digits,
        track: track.number,
        disc: track.disc_number,
        language: track.language_of_performance.join(", ").into(),
        year: track.album.date.year(),
        publisher: track.album.label.into(),
    };

    let mut path_string = path_template.resolve(&template_fields.sanitize_path())?;
    if let Some(max_len) = cfg.max_filename_len {
        path_string.truncate(max_len);
    }
    path_string.push('.');
    path_string.push_str(&profile.extension);
    let path = Path::new(&path_string);

    let parent = path
        .parent()
        .ok_or_else(|| eyre!("Specified path has no parent"))?;
    create_dir_all(parent).await?;

    if skip_existing && path.exists() {
        return Ok(false);
    }

    let filename = path.file_name().map_or_else(
        || path.to_string_lossy().to_string(),
        |v| v.to_string_lossy().to_string(),
    );

    let display_id = track.id.to_base62()?;

    let file = select_file(&track.files, allowed_formats)
        .ok_or_else(|| eyre!("Could not find a suitable file for track {display_id:?}"))?;

    let key = session.audio_key().request(track.id, file).await?;

    let cdn_url = CdnUrl::new(file).resolve_audio(session).await?;

    let resp = task::spawn_blocking(move || -> Result<Response> {
        Ok(ureq::get(cdn_url.try_get_url()?).call()?)
    })
    .await??;

    let size = resp
        .header("content-length")
        .ok_or_eyre("spotify cdn response didn't include content-length header")?
        .parse()?;

    let download_pb = ProgressBar::new(size);
    download_pb.set_style(pb_style);

    let mut audio_stream = download_pb.wrap_read(AudioDecrypt::new(Some(key), resp.into_reader()));

    let mut ffargs: Vec<Cow<'static, str>> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        "-".into(),
    ];

    let covers = track.album.covers.0;
    // keep the cover file in scope so that it only gets deleted after the download is finished
    let mut _cover: Option<TempFile>;

    let spclient = session.spclient();
    if !covers.is_empty() {
        let cover_id = covers.iter().max_by_key(|i| i.height).unwrap().id;
        if profile.cover_art {
            download_pb.set_message(format!(
                "(downloading cover art...) [{seq}/{track_count}] {filename}"
            ));
            let cover_data = spclient.get_image(&cover_id).await?;

            let mut cover_file = TempFile::new().await?;
            cover_file.write_all(&cover_data).await?;
            ffargs.push("-i".into());
            ffargs.push(cover_file.file_path().to_string_lossy().into_owned().into());
            _cover = Some(cover_file);
        } else if let Some(external_cover_art) = external_cover_art {
            let result = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(parent.join(external_cover_art))
                .await;

            match result {
                Ok(mut cover_file) => {
                    download_pb.set_message(format!(
                        "(downloading cover art...) [{seq}/{track_count}] {filename}"
                    ));
                    let cover_data = spclient.get_image(&cover_id).await?;

                    cover_file.write_all(&cover_data).await?;
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                Err(e) => bail!(e),
            }
        }
    }

    for arg in profile_ffargs {
        ffargs.push(arg.resolve(&template_fields)?.into());
    }

    ffargs.push(path_string.clone().into());

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

        // the first 167 bytes of the decrypted audio stream are useless
        // and they render the ogg file corrupted, so we skip them
        let mut garbage = [0u8; 167];
        audio_stream.read_exact(&mut garbage)?;

        io::copy(&mut audio_stream, &mut stdin)?;

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

    if let Err(e) = task.await? {
        let _ = fs::remove_file(path);
        Err(e)
    } else {
        Ok(true)
    }
}
