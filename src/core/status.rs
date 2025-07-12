//! This module provides a `StatusBar` struct that manages the display of
//! real-time application status in the terminal using the `indicatif` crate.
//! It handles displaying various metrics like FPS, network traffic, and the
//! status of different modules in a single, persistent line.

use std::{collections::VecDeque, sync::Arc, time::Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Manages a spinner-based status bar in the terminal.
pub struct StatusBar {
    /// A vector of messages to be displayed in the status bar for the current frame.
    messages: Vec<Arc<str>>,
    /// The `ProgressBar` from `indicatif` used to render the spinner and messages.
    spinner: ProgressBar,
    /// A queue to track the number of sent OSC packets over the last second.
    send_counter: VecDeque<(f32, Instant)>,
    /// A queue to track the timestamps of received OSC packets over the last second.
    recv_counter: VecDeque<Instant>,
    /// A queue to track the timestamps of application ticks (frames) over the last second.
    fps_counter: VecDeque<Instant>,
    /// The calculated ticks per second (FPS) of the main application loop.
    fps: f32,
    /// The time when the `StatusBar` was created, used for calculating uptime.
    start: Instant,
    /// The time elapsed since the last frame, used for time-delta calculations.
    pub last_frame_time: f32,
}

impl StatusBar {
    /// Creates a new `StatusBar`.
    ///
    /// # Arguments
    ///
    /// * `multi` - A `MultiProgress` manager from `indicatif` to which the new progress bar will be added.
    pub fn new(multi: &MultiProgress) -> Self {
        let spinner = multi.add(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::default_spinner().tick_chars("⠁⠂⠄⡀⡈⡐⡠⣀⣁⣂⣄⣌⣔⣤⣥⣦⣮⣶⣷⣿⡿⠿⢟⠟⡛⠛⠫⢋⠋⠍⡉⠉⠑⠡⢁"),
        );

        Self {
            messages: Vec::new(),
            spinner,
            send_counter: VecDeque::new(),
            recv_counter: VecDeque::new(),
            fps_counter: VecDeque::new(),
            start: Instant::now(),
            last_frame_time: 0f32,
            fps: 1f32,
        }
    }

    /// Records a new frame tick and updates the FPS calculation.
    /// It uses a sliding window of the last second to calculate the average FPS.
    pub fn trip_fps_counter(&mut self) {
        if let Some(last) = self.fps_counter.back() {
            self.last_frame_time = last.elapsed().as_secs_f32();
        }
        self.fps_counter.push_back(Instant::now());

        // Remove ticks older than 1 second from the front of the queue.
        while let Some(time) = self.fps_counter.front() {
            if time.elapsed().as_secs_f32() > 1. {
                self.fps_counter.pop_front();
            } else {
                break;
            }
        }

        let total_elapsed = self
            .fps_counter
            .front()
            .map(|time| time.elapsed().as_secs_f32())
            .unwrap_or(0f32);

        // Calculate FPS and add it to the display messages.
        self.fps = self.fps_counter.len() as f32 / total_elapsed;
        self.add_item(format!("TICK:{:.0}/s", self.fps).into());
    }

    /// Records that a packet has been received.
    /// It uses a sliding window to keep track of packets received in the last second.
    pub fn trip_recv_counter(&mut self) {
        self.recv_counter.push_back(Instant::now());
        // Remove timestamps older than 1 second.
        while let Some(time) = self.recv_counter.front() {
            if time.elapsed().as_secs_f32() > 1. {
                self.recv_counter.pop_front();
            } else {
                break;
            }
        }
    }

    /// Calculates and adds the received packets-per-second summary to the display messages.
    pub fn recv_summary(&mut self) {
        let total_elapsed = self
            .recv_counter
            .front()
            .map(|time| time.elapsed().as_secs_f32())
            .unwrap_or(0f32);

        self.add_item(
            format!(
                "RECV:{:.0}/s",
                self.recv_counter.len() as f32 / total_elapsed
            )
            .into(),
        );
    }

    /// Sets the number of packets sent in the last frame and updates the send rate calculation.
    pub fn set_sent_count(&mut self, count: f32) {
        self.send_counter.push_back((count, Instant::now()));

        // Remove entries older than 1 second.
        while let Some((_, time)) = self.send_counter.front() {
            if time.elapsed().as_secs_f32() > 1. {
                self.send_counter.pop_front();
            } else {
                break;
            }
        }

        let total_elapsed = self
            .send_counter
            .front()
            .map(|(_, time)| time.elapsed().as_secs_f32())
            .unwrap_or(0f32);

        // Sum all counts in the window and divide by the elapsed time to get the rate.
        let total = self
            .send_counter
            .iter()
            .map(|(count, _)| count)
            .sum::<f32>()
            / total_elapsed;

        self.add_item(format!("SEND:{:.1}/s", total).into());
    }

    /// Adds a string item to be displayed in the status bar for the current frame.
    pub fn add_item(&mut self, str: Arc<str>) {
        self.messages.push(str);
    }

    /// Updates the spinner with the collected messages for the current frame.
    /// After displaying, it clears the message buffer for the next frame.
    pub fn display(&mut self) {
        let uptime = self.start.elapsed().as_secs();
        if uptime >= 1 {
            // Join all messages with spaces and set it as the spinner's message.
            let str = self.messages.join("  ");
            self.spinner.set_message(str);
        } else {
            // Display "Initializing..." for the first second.
            self.spinner.set_message("Initializing...");
        }
        self.spinner.tick();
        self.messages.clear();
    }
}
