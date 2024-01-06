//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream configuration and that they
//! support the i32 sample format.
//!
//! Uses a delay of `latency_ms` milliseconds in case the default input and output streams are not
//! precisely synchronised.

// This software includes the work that is distributed in the Apache License 2.0
// Original source is:
//  https://github.com/RustAudio/cpal/blob/310160fbbf507bc27ef751a976550692540b6b9e/examples/feedback.rs
// This file has been modified by note_kdia to support ASIO devices.

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;

fn main() -> anyhow::Result<()> {
    type SampleFormat = i32;
    let channels = 2;
    let latency_ms: f32 = 150.0;

    // Select ASIO host
    #[cfg(target_os = "windows")]
    let host = cpal::host_from_id(cpal::HostId::Asio).expect("failed to initialise ASIO host");

    // Find device
    let asio_device = host
        .default_output_device()
        .expect("failed to find asio device");

    println!("Using asio device: \"{}\"", asio_device.name()?);

    // Check if ASIO device's sample format is i32
    // If this assertion fails, change SampleFormat type declared above
    let mut supported_configs_range = asio_device
        .supported_input_configs()
        .expect("error while querying configs");
    let supported_config = supported_configs_range
        .next()
        .unwrap()
        .with_max_sample_rate();
    assert_eq!(
        supported_config.sample_format().to_string(),
        std::any::type_name::<SampleFormat>()
    );

    // We'll try and use the same configuration between streams to keep it simple.
    let config_default: cpal::StreamConfig = asio_device.default_input_config()?.into();
    let config = cpal::StreamConfig {
        channels,
        sample_rate: config_default.sample_rate,
        buffer_size: config_default.buffer_size,
    };

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (latency_ms / 1_000.0) * config.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * config.channels as usize;

    // The buffer to share samples
    let ring = HeapRb::<SampleFormat>::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        producer.push(0).unwrap();
    }

    let input_data_fn = move |data: &[SampleFormat], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            if producer.push(sample).is_err() {
                output_fell_behind = true;
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency",);
        }
    };

    let output_data_fn = move |data: &mut [SampleFormat], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0
                }
            };
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency",);
        }
    };

    // Build streams
    println!(
        "Attempting to build both streams with i32 samples and `{:?}`.",
        config
    );

    let input_stream = asio_device.build_input_stream(&config, input_data_fn, err_fn, None)?;
    let output_stream = asio_device.build_output_stream(&config, output_data_fn, err_fn, None)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        latency_ms
    );
    input_stream.play()?;
    output_stream.play()?;

    // Main thread does not wait for streams to close.
    println!("Feedback for 3 secs");
    std::thread::sleep(Duration::from_secs(3));

    drop(input_stream);
    drop(output_stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
