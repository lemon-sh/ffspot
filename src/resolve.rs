use color_eyre::Result;
use indicatif::ProgressBar;
use librespot::{
    core::{Session, SpotifyId},
    metadata::{Album, Metadata, Playlist, Track},
};

async fn resolve_track(session: &Session, id: &SpotifyId) -> Result<Track> {
    let track = Track::get(session, id).await?;
    if let Some(alternative) = track.alternatives.first() {
        Ok(Track::get(session, alternative).await?)
    } else {
        Ok(track)
    }
}

async fn resolve_track_ids(
    session: &Session,
    ids: impl Iterator<Item = &SpotifyId>,
    pb: ProgressBar,
) -> Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for id in pb.wrap_iter(ids) {
        tracks.push(Track::get(session, id).await?);
    }
    Ok(tracks)
}

pub async fn resolve_tracks(
    resource_type: &str,
    resource_id: &str,
    session: &Session,
    pb: ProgressBar,
) -> Result<Vec<Track>> {
    let id = SpotifyId::from_base62(resource_id)?;
    match resource_type {
        "track" => {
            pb.set_length(1);
            let track = resolve_track(session, &id).await?;
            pb.finish_using_style();
            Ok(vec![track])
        }
        "album" => {
            let album = Album::get(session, &id).await?;
            pb.set_length(album.tracks().count() as u64);
            Ok(resolve_track_ids(session, album.tracks(), pb).await?)
        }
        "playlist" => {
            let playlist = Playlist::get(session, &id).await?;
            pb.set_length(playlist.tracks().count() as u64);
            Ok(resolve_track_ids(session, playlist.tracks(), pb).await?)
        }
        _ => panic!("Unknown resource type {resource_type:?}. The regex shouldn't have matched."),
    }
}
