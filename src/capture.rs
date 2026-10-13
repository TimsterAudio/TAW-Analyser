//! Listen-only audio for the Analyser: what an output is playing, or
//! an input such as a microphone. Nothing is ever played back, so nothing
//! can feed back.
//!
//! - Linux: PipeWire. An output is heard through its monitor.
//! - Windows: WASAPI. An output is heard through loopback capture.
//! - Elsewhere, and for inputs on Windows: cpal input devices.

#[cfg(not(target_os = "linux"))]
use cpal::traits::StreamTrait;
use cpal::traits::{DeviceTrait, HostTrait};
#[cfg(not(target_os = "linux"))]
use cpal::HostId;
#[cfg(not(target_os = "linux"))]
use cpal::{FromSample, SampleFormat, SizedSample};
#[cfg(not(target_os = "linux"))]
use std::str::FromStr;
use std::sync::atomic::AtomicU32;
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{mpsc, Arc};
use taw_panel::analyzer::AnalyzerTap;

/// Something to listen to. `id` is stable between runs, so it can be saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub id: String,
    pub label: String,
}

/// The source used when none is chosen: whatever the default output plays.
pub const DEFAULT_OUTPUT: &str = "output:";

/// A running capture. Dropping it stops listening.
pub struct Capture {
    pub description: String,
    /// The capture's sample rate, once known (0 until then).
    pub sample_rate: Arc<AtomicU32>,
    _stop: Box<dyn Send>,
}

pub fn sources() -> Vec<Source> {
    let mut v = vec![Source {
        id: DEFAULT_OUTPUT.into(),
        label: "Default output (what's playing)".into(),
    }];
    #[cfg(target_os = "linux")]
    v.extend(pw_sources());
    #[cfg(not(target_os = "linux"))]
    v.extend(cpal_sources());
    v
}

pub fn start(id: &str, tap: AnalyzerTap) -> Result<Capture, String> {
    let rate = Arc::new(AtomicU32::new(0));
    #[cfg(target_os = "linux")]
    let (description, stop) = pw::start(id, tap, rate.clone())?;
    #[cfg(not(target_os = "linux"))]
    let (description, stop) = cpal_start(id, tap, rate.clone())?;
    Ok(Capture {
        description,
        sample_rate: rate,
        _stop: stop,
    })
}

/// "output:<id>" or "input:<id>"; an empty id means the default.
fn split(id: &str) -> (bool, Option<&str>) {
    if let Some(rest) = id.strip_prefix("input:") {
        (false, (!rest.is_empty()).then_some(rest))
    } else {
        let rest = id.strip_prefix("output:").unwrap_or("");
        (true, (!rest.is_empty()).then_some(rest))
    }
}

// ------------------------------------------------------------------- cpal

#[cfg(not(target_os = "linux"))]
fn cpal_host() -> Result<cpal::Host, String> {
    if cfg!(windows) {
        host("WASAPI")
    } else {
        Ok(cpal::default_host())
    }
}

#[cfg(not(target_os = "linux"))]
fn cpal_sources() -> Vec<Source> {
    let Ok(host) = cpal_host() else {
        return Vec::new();
    };
    let Ok(all) = host.devices() else {
        return Vec::new();
    };
    let loopback = cfg!(windows);
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    for d in all {
        let Ok(id) = d.id() else { continue };
        let name = device_name(&d);
        if d.supports_output() {
            if loopback {
                outputs.push(Source {
                    id: format!("output:{id}"),
                    label: format!("{name} (what's playing)"),
                });
            }
        } else if d.supports_input() {
            inputs.push(Source {
                id: format!("input:{id}"),
                label: name,
            });
        }
    }
    outputs.extend(inputs);
    outputs
}

#[cfg(not(target_os = "linux"))]
fn cpal_start(
    id: &str,
    tap: AnalyzerTap,
    rate: Arc<AtomicU32>,
) -> Result<(String, Box<dyn Send>), String> {
    let (output, dev_id) = split(id);
    let id = dev_id.map(str::to_string);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    // cpal streams may not move between threads: one thread owns it.
    let thread = std::thread::Builder::new()
        .name("eq-box-capture".into())
        .spawn(move || {
            let opened = (|| {
                let host = cpal_host()?;
                if output && !cfg!(windows) {
                    return Err("Listening to an output needs Windows or PipeWire".to_string());
                }
                let dev = find_device(&host, id.as_deref(), output)?;
                let cfg = if output {
                    dev.default_output_config()
                } else {
                    dev.default_input_config()
                }
                .map_err(|e| e.to_string())?;
                rate.store(cfg.sample_rate(), Relaxed);
                let ch = cfg.channels() as usize;
                let stream = match cfg.sample_format() {
                    SampleFormat::F32 => build::<f32>(&dev, &cfg.config(), ch, tap),
                    SampleFormat::I16 => build::<i16>(&dev, &cfg.config(), ch, tap),
                    SampleFormat::I32 => build::<i32>(&dev, &cfg.config(), ch, tap),
                    other => Err(format!("{other} audio isn't supported")),
                }?;
                stream.play().map_err(|e| e.to_string())?;
                let what = match (output, id.is_some()) {
                    (true, false) => "DEFAULT OUTPUT",
                    (true, true) => "OUTPUT",
                    (false, false) => "DEFAULT INPUT",
                    (false, true) => "INPUT",
                };
                Ok((stream, what.to_string()))
            })();
            match opened {
                Ok((stream, desc)) => {
                    let _ = ready_tx.send(Ok(desc));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let desc = ready_rx
        .recv()
        .map_err(|_| "Listening stopped unexpectedly".to_string())??;
    Ok((desc, Box::new(Stop::new(stop_tx, thread))))
}

#[cfg(not(target_os = "linux"))]
fn build<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    ch: usize,
    mut tap: AnalyzerTap,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut buf: Vec<f32> = Vec::with_capacity(8192);
    dev.build_input_stream(
        *cfg,
        move |data: &[T], _| {
            buf.clear();
            buf.extend(data.iter().map(|s| f32::from_sample_(*s)));
            tap.push(&buf, ch);
        },
        |e| eprintln!("capture error: {e}"),
        None,
    )
    .map_err(|e| format!("Couldn't listen to the device: {e}"))
}

struct Stop {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Stop {
    #[allow(dead_code)] // not used by the PipeWire path
    fn new(stop: mpsc::Sender<()>, thread: std::thread::JoinHandle<()>) -> Self {
        Stop {
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Stop {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// --------------------------------------------------------------- PipeWire

#[cfg(target_os = "linux")]
fn pw_sources() -> Vec<Source> {
    let Ok(host) = cpal::host_from_id(cpal::HostId::PipeWire) else {
        return Vec::new();
    };
    let Ok(all) = host.devices() else {
        return Vec::new();
    };
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    for d in all {
        let Ok(id) = d.id() else { continue };
        let node = id.id().to_string();
        if node.ends_with("_default") || node.starts_with("default_") {
            continue;
        }
        let name = device_name(&d);
        if d.supports_output() {
            outputs.push(Source {
                id: format!("output:{node}"),
                label: format!("{name} (what's playing)"),
            });
        } else if d.supports_input() {
            inputs.push(Source {
                id: format!("input:{node}"),
                label: name,
            });
        }
    }
    outputs.extend(inputs);
    outputs
}

#[cfg(target_os = "linux")]
mod pw {
    use super::{split, Stop};
    use pipewire as pw;
    use pw::{properties::properties, spa};
    use spa::param::audio::{AudioFormat, AudioInfoRaw};
    use spa::pod::Pod;
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    use taw_panel::analyzer::AnalyzerTap;

    const CHANNELS: usize = 2;
    const STRIDE: usize = CHANNELS * std::mem::size_of::<f32>();

    struct PwStop {
        quit: Option<pw::channel::Sender<()>>,
        _thread: Stop,
    }

    impl Drop for PwStop {
        fn drop(&mut self) {
            if let Some(q) = self.quit.take() {
                let _ = q.send(());
            }
        }
    }

    pub fn start(
        id: &str,
        tap: AnalyzerTap,
        rate: Arc<AtomicU32>,
    ) -> Result<(String, Box<dyn Send>), String> {
        let (output, target) = split(id);
        let target = target.map(str::to_string);
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let desc = match (&target, output) {
            (None, true) => "DEFAULT OUTPUT",
            (Some(_), true) => "OUTPUT",
            (None, false) => "DEFAULT INPUT",
            (Some(_), false) => "INPUT",
        }
        .to_string();
        let thread = std::thread::Builder::new()
            .name("eq-box-capture".into())
            .spawn(move || {
                if let Err(e) = run(output, target, tap, rate, quit_rx, &ready_tx) {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        let stop = PwStop {
            quit: Some(quit_tx),
            _thread: Stop {
                stop: None,
                thread: Some(thread),
            },
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok((desc, Box::new(stop))),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("PipeWire didn't answer. Is it running?".into()),
        }
    }

    fn format_pod() -> Vec<u8> {
        let mut info = AudioInfoRaw::new();
        info.set_format(AudioFormat::F32LE);
        info.set_channels(CHANNELS as u32);
        let mut position = [0; spa::param::audio::MAX_CHANNELS];
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        info.set_position(position);
        pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(pw::spa::pod::Object {
                type_: spa::sys::SPA_TYPE_OBJECT_Format,
                id: spa::sys::SPA_PARAM_EnumFormat,
                properties: info.into(),
            }),
        )
        .expect("audio format pod")
        .0
        .into_inner()
    }

    struct Data {
        tap: AnalyzerTap,
        scratch: Vec<f32>,
    }

    fn run(
        output: bool,
        target: Option<String>,
        tap: AnalyzerTap,
        rate: Arc<AtomicU32>,
        quit_rx: pw::channel::Receiver<()>,
        ready: &mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        pw::init();
        let pw_err = |what: &str| {
            let what = what.to_string();
            move |e: pw::Error| format!("{what}: {e}")
        };
        let mainloop =
            pw::main_loop::MainLoopRc::new(None).map_err(pw_err("PipeWire main loop"))?;
        let context =
            pw::context::ContextRc::new(&mainloop, None).map_err(pw_err("PipeWire context"))?;
        let core = context
            .connect_rc(None)
            .map_err(pw_err("Couldn't connect to PipeWire (is it running?)"))?;
        let weak = mainloop.downgrade();
        let _quit = quit_rx.attach(mainloop.loop_(), move |_| {
            if let Some(ml) = weak.upgrade() {
                ml.quit();
            }
        });

        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "DSP",
            *pw::keys::NODE_NAME => "sp_10",
            *pw::keys::NODE_DESCRIPTION => "SP-10 Spectrum Analyser",
            "audio.position" => "FL,FR",
        };
        if output {
            // Listen to an output's monitor rather than a microphone.
            props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
        }
        if let Some(t) = &target {
            props.insert(*pw::keys::TARGET_OBJECT, t.as_str());
        }
        let stream = pw::stream::StreamBox::new(&core, "SP-10 Spectrum Analyser", props)
            .map_err(pw_err("Couldn't create the capture stream"))?;

        let _format = stream
            .add_local_listener_with_user_data(())
            .param_changed(move |_, _, id, param| {
                let Some(param) = param else { return };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let mut info = AudioInfoRaw::new();
                if info.parse(param).is_ok() && info.rate() > 0 {
                    rate.store(info.rate(), Relaxed);
                }
            })
            .register()
            .map_err(pw_err("PipeWire listener"))?;
        let _process = stream
            .add_local_listener_with_user_data(Data {
                tap,
                scratch: vec![0.0; 8192 * CHANNELS],
            })
            .process(|stream, data| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(d) = datas.first_mut() else { return };
                let offset = d.chunk().offset() as usize;
                let size = d.chunk().size() as usize;
                let Some(bytes) = d.data() else { return };
                let end = (offset + size).min(bytes.len());
                let bytes = &bytes[offset.min(end)..end];
                let n = (bytes.len() / STRIDE * CHANNELS).min(data.scratch.len());
                let buf = &mut data.scratch[..n];
                for (s, b) in buf.iter_mut().zip(bytes.as_chunks::<4>().0) {
                    *s = f32::from_le_bytes(*b);
                }
                data.tap.push(buf, CHANNELS);
            })
            .register()
            .map_err(pw_err("PipeWire listener"))?;

        let flags = pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS;
        let pod = format_pod();
        let mut params = [Pod::from_bytes(&pod).expect("format pod")];
        stream
            .connect(spa::utils::Direction::Input, None, flags, &mut params)
            .map_err(pw_err("Couldn't start listening"))?;
        let _ = ready.send(Ok(()));
        mainloop.run();
        let _ = stream.disconnect();
        Ok(())
    }
}

// ---------------------------------------------------------------- cpal

#[cfg(not(target_os = "linux"))]
fn host(name: &str) -> Result<cpal::Host, String> {
    let id = HostId::from_str(name).map_err(|_| format!("Unknown audio system \"{name}\""))?;
    cpal::host_from_id(id).map_err(|e| format!("{name} isn't available: {e}"))
}

fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Unnamed device".into())
}

#[cfg(not(target_os = "linux"))]
fn find_device(host: &cpal::Host, id: Option<&str>, output: bool) -> Result<cpal::Device, String> {
    match id {
        Some(id) => {
            let parsed = cpal::DeviceId::from_str(id).map_err(|e| e.to_string())?;
            host.device_by_id(&parsed)
                .ok_or_else(|| "The chosen device isn't connected any more".to_string())
        }
        None => if output {
            host.default_output_device()
        } else {
            host.default_input_device()
        }
        .ok_or_else(|| {
            format!(
                "No default {} device",
                if output { "output" } else { "input" }
            )
        }),
    }
}
