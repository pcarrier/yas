//! Native YAS Media player controls.

use yas_wire::{Encode, Extensions, family, media};

use crate::{cli::MediaCommand, yas_native::NativeClient};

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: Option<MediaCommand>,
) -> Result<(), String> {
    let command = command.unwrap_or(MediaCommand::List);
    if matches!(command, MediaCommand::List) {
        return list(on, hub).await;
    }

    let (action, requested_player) =
        action_and_player(&command).ok_or_else(|| "missing media player action".to_string())?;
    let mut client = NativeClient::connect(on, hub).await?;
    let players = player_records(&mut client).await?;
    let player = select_player(&players, requested_player)?;
    let request = media::PlayerAction {
        player_handle: player.player_handle,
        operation_id: operation_id(),
        action,
        value: 0,
        extensions: Extensions::default(),
    };
    let body = client
        .request(
            family::MEDIA,
            media::request_kind::PLAYER_ACTION,
            request
                .encode()
                .map_err(|error| format!("YAS wire error: {error}"))?,
            true,
        )
        .await?;
    if !body.is_empty() {
        return Err(format!(
            "YAS Media PLAYER_ACTION returned an unexpected {}-byte body",
            body.len()
        ));
    }
    Ok(())
}

async fn list(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let mut players = player_records(&mut client).await?;
    players.sort_by_key(|player| player.player_handle);

    println!("ID\tSTATE\tACTIVE\tART\tPLAYER\tTITLE\tARTIST");
    for player in players {
        let active = player
            .active()
            .map_err(|error| format!("invalid YAS Media player state: {error}"))?;
        let artwork = if player
            .album_art_url()
            .map_err(|error| format!("invalid YAS Media player artwork: {error}"))?
            .is_some()
        {
            "url"
        } else if player.extensions.0.iter().any(|extension| {
            extension.tag == yas_wire::schema::media::PLAYER_ALBUM_ART_HASH_EXTENSION as u16
        }) {
            "asset"
        } else {
            "-"
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            player.player_handle,
            state_name(player.state),
            match active {
                Some(true) => "yes",
                Some(false) => "no",
                None => "-",
            },
            artwork,
            table_cell(&player.identity),
            table_cell(&player.title),
            table_cell(&player.artist),
        );
    }
    Ok(())
}

fn action_and_player(command: &MediaCommand) -> Option<(u16, Option<u64>)> {
    let (action, player) = match command {
        MediaCommand::List => return None,
        MediaCommand::Play(target) => (yas_wire::schema::media::PLAYER_ACTION_PLAY, &target.player),
        MediaCommand::Pause(target) => {
            (yas_wire::schema::media::PLAYER_ACTION_PAUSE, &target.player)
        }
        MediaCommand::Toggle(target) => (
            yas_wire::schema::media::PLAYER_ACTION_PLAY_PAUSE,
            &target.player,
        ),
        MediaCommand::Stop(target) => (yas_wire::schema::media::PLAYER_ACTION_STOP, &target.player),
        MediaCommand::Next(target) => (yas_wire::schema::media::PLAYER_ACTION_NEXT, &target.player),
        MediaCommand::Previous(target) => (
            yas_wire::schema::media::PLAYER_ACTION_PREVIOUS,
            &target.player,
        ),
        MediaCommand::Raise(target) => {
            (yas_wire::schema::media::PLAYER_ACTION_RAISE, &target.player)
        }
    };
    Some((action as u16, *player))
}

async fn player_records(client: &mut NativeClient) -> Result<Vec<media::PlayerRecord>, String> {
    let records = client
        .snapshot(family::MEDIA)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Media family".to_string())?;
    let mut players = Vec::new();
    for record in records {
        let mutation = media::decode_state_record(&record)
            .map_err(|error| format!("invalid YAS Media state: {error}"))?;
        if let media::StateMutation::Complete(media::CompleteEntity::Player(player)) = mutation {
            players.push(player);
        }
    }
    Ok(players)
}

fn select_player(
    players: &[media::PlayerRecord],
    requested: Option<u64>,
) -> Result<&media::PlayerRecord, String> {
    if let Some(handle) = requested {
        if handle == 0 {
            return Err("media player ID must be nonzero".to_string());
        }
        return players
            .iter()
            .find(|player| player.player_handle == handle)
            .ok_or_else(|| format!("media player {handle} not found"));
    }

    for player in players {
        if player
            .active()
            .map_err(|error| format!("invalid YAS Media player state: {error}"))?
            == Some(true)
        {
            return Ok(player);
        }
    }
    players
        .iter()
        .find(|player| player.state == yas_wire::schema::media::PLAYER_PLAYING as u16)
        .or_else(|| players.iter().min_by_key(|player| player.player_handle))
        .ok_or_else(|| "no desktop media players".to_string())
}

fn state_name(state: u16) -> &'static str {
    match u64::from(state) {
        yas_wire::schema::media::PLAYER_STOPPED => "stopped",
        yas_wire::schema::media::PLAYER_PAUSED => "paused",
        yas_wire::schema::media::PLAYER_PLAYING => "playing",
        _ => "unknown",
    }
}

fn table_cell(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn operation_id() -> [u8; 16] {
    let mut value: [u8; 16] = rand::random();
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(handle: u64, state: u16, active: Option<bool>) -> media::PlayerRecord {
        let mut extensions = Extensions::default();
        if let Some(active) = active {
            extensions.0.push(media::player_active_extension(active));
        }
        media::PlayerRecord {
            player_handle: handle,
            revision: 1,
            state,
            flags: 0,
            position_us: 0,
            duration_us: -1,
            identity: format!("player-{handle}"),
            desktop_entry: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            extensions,
        }
    }

    #[test]
    fn default_player_prefers_active_then_playing_then_lowest_handle() {
        let paused = yas_wire::schema::media::PLAYER_PAUSED as u16;
        let playing = yas_wire::schema::media::PLAYER_PLAYING as u16;
        let players = vec![
            player(4, paused, Some(false)),
            player(8, playing, Some(false)),
            player(9, paused, Some(true)),
        ];
        assert_eq!(select_player(&players, None).unwrap().player_handle, 9);

        let players = vec![player(4, paused, None), player(8, playing, None)];
        assert_eq!(select_player(&players, None).unwrap().player_handle, 8);

        let players = vec![player(8, paused, None), player(4, paused, None)];
        assert_eq!(select_player(&players, None).unwrap().player_handle, 4);
    }

    #[test]
    fn explicit_player_is_exact() {
        let paused = yas_wire::schema::media::PLAYER_PAUSED as u16;
        let players = vec![player(4, paused, None), player(8, paused, None)];
        assert_eq!(select_player(&players, Some(8)).unwrap().player_handle, 8);
        assert_eq!(
            select_player(&players, Some(7)).unwrap_err(),
            "media player 7 not found"
        );
    }
}
