//! Two-way "Call": the viewer's voice, arriving via WHIP at mediamtx
//! (path /talk), decoded and played into the room -- plus the app-level
//! /api/talk state (two-way on/off + heartbeat) a call's lifetime rides
//! on. The viewer's own WHIP publish and its WHEP subscribe to the room's
//! return audio talk directly to mediamtx (:8889) and never touch this
//! process; this module is only the "room hears the caller" leg.
//!
//! Mirrors broadcast-api's /api/talk + "ffmpeg -f alsa" design (see
//! aibridge/handoffs/2026-09-09-notch-filter-system-and-audio-hud-work.md)
//! adapted for a real Windows gap broadcast-api doesn't have: the bundled
//! ffmpeg build has zero audio OUTPUT devices (`ffmpeg -devices` lists
//! none -- no dshow/wasapi muxer, unlike Linux's ALSA sink), so ffmpeg
//! only DECODES here (stdout raw PCM) and cpal (WASAPI) owns the actual
//! device write. Same spawn-ffmpeg-consume-raw-output shape
//! video_preview.rs already uses for the camera preview, just audio
//! instead of pixels -- not a new pattern in this codebase.
//!
//! GUI "return view" (the local desktop preview expanding into a call
//! display when one goes active) is explicitly NOT built here -- 7elwe's
//! GUI has no existing expand-style surface to repurpose the way
//! Tanzania's SSH TUI does, so that's its own design pass. This module
//! only has to make `Talk::is_two_way()` available for whenever that
//! lands.

use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const TALK_RTSP_URL: &str = "rtsp://127.0.0.1:8554/talk";
/// One rate/channel-count everywhere in the talk leg -- matches
/// pipeline.rs's own AUDIO_SAMPLE_RATE/AUDIO_CHANNELS -- so there's no
/// resampling stage anywhere in this chain.
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_BYTES: usize = 2 * CHANNELS as usize; // 16-bit samples x channels
/// mediamtx accepts a WHIP publisher before its first packet arrives, so
/// an ffmpeg started the instant a call goes active can find the path
/// empty and exit almost immediately. A short, tight retry rides that out
/// -- same reasoning as broadcast-api's own talk-player supervisor, which
/// rate-limits to one respawn per 0.5s rather than a fixed warm-up delay.
const RETRY_BACKOFF: Duration = Duration::from_millis(500);
/// No GET /api/talk poll within this long ends an abandoned call (tab
/// closed, crash) independently of whether the playback process is still
/// alive. ~4x the client's 4s poll interval.
const HEARTBEAT_GRACE: Duration = Duration::from_secs(15);

pub struct Talk {
    ffmpeg: PathBuf,
    /// For the operator-tunable outbound gain and the test-only speaker
    /// mute -- see audio_settings.rs. Read live on each (re)spawn and in
    /// the audio callback, so a Settings change takes effect without
    /// restarting anything here.
    state: Arc<crate::state::AppState>,
    two_way: AtomicBool,
    last_heartbeat: StdMutex<Option<Instant>>,
}

impl Talk {
    pub fn new(ffmpeg: PathBuf, state: Arc<crate::state::AppState>) -> Arc<Self> {
        let t = Arc::new(Self {
            ffmpeg,
            state,
            two_way: AtomicBool::new(false),
            last_heartbeat: StdMutex::new(None),
        });
        spawn_heartbeat_watchdog(t.clone());
        spawn_playback_supervisor(t.clone());
        t
    }

    pub fn is_two_way(&self) -> bool {
        self.two_way.load(Ordering::Relaxed)
    }

    /// POST /api/talk body handler. "two-way" starts (or refreshes) a
    /// call; anything else -- including the explicit "false" hangup body
    /// -- ends it immediately rather than waiting on the heartbeat grace
    /// period, mirroring broadcast-api's own POST semantics.
    pub fn set(&self, body: &str) -> bool {
        let on = body.trim() == "two-way";
        self.two_way.store(on, Ordering::Relaxed);
        *self.last_heartbeat.lock().unwrap() = if on { Some(Instant::now()) } else { None };
        on
    }

    /// GET /api/talk: doubles as the client's heartbeat (resets the grace
    /// clock) and a status read for its own UI.
    pub fn poll(&self) -> bool {
        let on = self.two_way.load(Ordering::Relaxed);
        if on {
            *self.last_heartbeat.lock().unwrap() = Some(Instant::now());
        }
        on
    }
}

fn spawn_heartbeat_watchdog(t: Arc<Talk>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let stale = {
                let g = t.last_heartbeat.lock().unwrap();
                matches!(*g, Some(last) if last.elapsed() > HEARTBEAT_GRACE)
            };
            if stale {
                t.two_way.store(false, Ordering::Relaxed);
                *t.last_heartbeat.lock().unwrap() = None;
                crate::launch_log("talk: heartbeat grace expired, ending call");
            }
        }
    });
}

/// A dedicated OS thread, not a tokio task: cpal's Stream owns thread-
/// affine WASAPI/COM objects, so the stream is created, lives, and is
/// dropped all on the one thread that made it (see run_playback_once).
fn spawn_playback_supervisor(t: Arc<Talk>) {
    std::thread::spawn(move || loop {
        if !t.two_way.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        if let Err(e) = run_playback_once(&t) {
            eprintln!("talk: playback error: {e}");
            crate::launch_log(&format!("talk: playback error: {e}"));
        }
        std::thread::sleep(RETRY_BACKOFF);
    });
}

fn run_playback_once(t: &Talk) -> Result<(), String> {
    let mut cmd = std::process::Command::new(&t.ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-rtsp_transport",
        "tcp",
        "-i",
        TALK_RTSP_URL,
    ]);
    // Operator-tunable outbound gain (viewer -> room speaker). Its
    // ceiling is deliberately lower than the inbound gain's: this is the
    // acoustic-feedback direction. Read at spawn, so a Settings change
    // lands on the next respawn rather than needing a restart here.
    let gain = t.state.audio.talk_gain_db();
    if gain != 0.0 {
        cmd.args(["-af", &format!("volume={gain}dB")]);
    }
    cmd.args([
        "-f",
        "s16le",
        "-ar",
        &SAMPLE_RATE.to_string(),
        "-ac",
        &CHANNELS.to_string(),
        "pipe:1",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg: no stdout pipe")?;

    // Bounded so a stalled audio callback applies backpressure to the
    // reader rather than this thread buffering an unbounded amount of
    // stale audio in memory.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(64);
    let stream = build_output_stream(rx, t.state.clone())?;
    stream.play().map_err(|e| format!("cpal play: {e}"))?;

    // Carries any trailing partial frame across read() calls -- read()
    // boundaries don't line up with FRAME_BYTES, so without this the
    // stereo L/R phase would drift out of alignment over time.
    let mut leftover: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if !t.two_way.load(Ordering::Relaxed) {
            break;
        }
        match stdout.read(&mut buf) {
            Ok(0) => break, // ffmpeg exited / pipe closed
            Ok(n) => {
                let mut chunk = std::mem::take(&mut leftover);
                chunk.extend_from_slice(&buf[..n]);
                let usable = chunk.len() - (chunk.len() % FRAME_BYTES);
                let samples: Vec<i16> = chunk[..usable]
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
                leftover = chunk[usable..].to_vec();
                if tx.send(samples).is_err() {
                    break; // audio callback/stream gone
                }
            }
            Err(e) => return Err(format!("read ffmpeg stdout: {e}")),
        }
    }
    drop(stream);
    let _ = child.kill();
    Ok(())
}

fn build_output_stream(
    rx: std::sync::mpsc::Receiver<Vec<i16>>,
    state: Arc<crate::state::AppState>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default audio output device")?;
    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    // Pull-side ring the audio callback drains into contiguous samples
    // from. try_recv only -- never blocks the realtime audio thread.
    let mut ring: std::collections::VecDeque<i16> = std::collections::VecDeque::new();

    device
        .build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                while ring.len() < data.len() {
                    match rx.try_recv() {
                        Ok(chunk) => ring.extend(chunk),
                        Err(_) => break,
                    }
                }
                // Test-only speaker mute: keep draining the ring so the
                // stream stays in sync and the call keeps running --
                // only the physical output goes silent, which is the
                // whole point (verify a call end to end without the
                // room hearing it).
                let muted = state.audio.talk_mute_speaker();
                for sample in data.iter_mut() {
                    let s = ring.pop_front().unwrap_or(0); // underrun: silence, not garbage
                    *sample = if muted { 0 } else { s };
                }
            },
            |err| eprintln!("talk: cpal stream error: {err}"),
            None,
        )
        .map_err(|e| format!("build_output_stream: {e}"))
}
