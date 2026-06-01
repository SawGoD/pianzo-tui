//! Синтез и проигрывание мелодии как реального звука (фортепианные тоны).
//!
//! Один постоянный поток держит аудио-выход и принимает мелодии через канал.
//! Громкость берётся из общего `Arc<Mutex<f32>>` и применяется в реальном
//! времени, остановка — через общий со всем приложением флаг `stop`.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};

use crate::debug;
use crate::parser::PitchEvent;

const SAMPLE_RATE: u32 = 44_100;
/// Сколько секунд «звенит» каждая нота (с затуханием).
const NOTE_DUR: f32 = 0.6;

/// Запрос на проигрывание: группы одновременных нот с паузами.
pub struct AudioRequest {
    pub groups: Vec<PitchEvent>,
}

/// Сообщения от аудио-потока.
#[derive(Debug)]
pub enum AudioMsg {
    Finished,
    Stopped,
    Error(String),
}

fn midi_to_freq(midi: u8) -> f32 {
    440.0 * 2f32.powf((midi as f32 - 69.0) / 12.0)
}

/// Синтезирует всю мелодию в один PCM-буфер (моно, f32).
fn render(groups: &[PitchEvent]) -> Vec<f32> {
    let sr = SAMPLE_RATE as f32;

    // Время начала каждой группы — накопленная сумма пауз.
    let mut onsets = Vec::with_capacity(groups.len());
    let mut t = 0.0f32;
    for (_, pause) in groups {
        onsets.push(t);
        t += (*pause as f32).max(0.0);
    }

    let total = t + NOTE_DUR + 0.2;
    let len = (total * sr) as usize + 1;
    let mut buf = vec![0f32; len];

    let dur_samples = (NOTE_DUR * sr) as usize;

    for (gi, (notes, _)) in groups.iter().enumerate() {
        let start = (onsets[gi] * sr) as usize;
        for &midi in notes {
            let f = midi_to_freq(midi);
            for k in 0..dur_samples {
                let idx = start + k;
                if idx >= len {
                    break;
                }
                let tt = k as f32 / sr;
                // Экспоненциальное затухание + лёгкая атака — «фортепианность».
                let attack = (tt / 0.005).min(1.0);
                let env = attack * (-tt * 4.5).exp();
                let s = (2.0 * PI * f * tt).sin()
                    + 0.30 * (2.0 * PI * 2.0 * f * tt).sin()
                    + 0.15 * (2.0 * PI * 3.0 * f * tt).sin();
                buf[idx] += env * s;
            }
        }
    }

    // Умеренная амплитуда на голос; пик ограничиваем мягким клиппингом.
    for x in &mut buf {
        *x = (*x * 0.18).clamp(-1.0, 1.0);
    }
    buf
}

/// Запускает постоянный аудио-поток.
pub fn spawn(
    rx: Receiver<AudioRequest>,
    stop: Arc<AtomicBool>,
    tx: Sender<AudioMsg>,
    volume: Arc<Mutex<f32>>,
) {
    thread::spawn(move || {
        let (_stream, handle) = match OutputStream::try_default() {
            Ok(v) => v,
            Err(e) => {
                debug::log(&format!("audio: нет аудио-выхода: {e}"));
                let msg = format!("Нет аудио-выхода: {e}");
                while rx.recv().is_ok() {
                    let _ = tx.send(AudioMsg::Error(msg.clone()));
                }
                return;
            }
        };
        debug::log("audio: выход готов, поток ждёт мелодии");

        while let Ok(req) = rx.recv() {
            let samples = render(&req.groups);
            debug::log(&format!(
                "audio: рендер {} групп, {} сэмплов",
                req.groups.len(),
                samples.len()
            ));

            let sink = match Sink::try_new(&handle) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(AudioMsg::Error(format!("Не создать вывод: {e}")));
                    continue;
                }
            };
            let cur_vol = volume.lock().map(|v| *v).unwrap_or(0.5);
            sink.set_volume(cur_vol);
            sink.append(SamplesBuffer::new(1, SAMPLE_RATE, samples));

            let mut stopped = false;
            while !sink.empty() {
                if stop.load(Ordering::Relaxed) {
                    sink.stop();
                    stopped = true;
                    break;
                }
                // Живое изменение громкости.
                if let Ok(v) = volume.lock() {
                    sink.set_volume(*v);
                }
                thread::sleep(Duration::from_millis(20));
            }

            if stopped {
                debug::log("audio: остановлено");
                let _ = tx.send(AudioMsg::Stopped);
            } else {
                debug::log("audio: завершено");
                let _ = tx.send(AudioMsg::Finished);
            }
        }
    });
}
