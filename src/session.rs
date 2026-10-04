//! Owner-thread transport protocol. Effects are completed by an engine adapter;
//! requesting Stop or Exit alone never proves that a producer has quiesced.
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Stopped,
    Starting,
    Recording,
    Finalizing,
    Playing,
    Paused,
    StoppingPlayback,
    Exited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    PrepareCapture(u64),
    QuiesceAndFinalize(u64),
    StartPlayback,
    PausePlayback,
    ResumePlayback,
    QuiescePlayback,
    ReturnToCaller,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedTake {
    pub path: PathBuf,
    pub complete: bool,
    pub detail: String,
}

#[derive(Debug)]
pub struct Session {
    transport: Transport,
    exit_confirmation: bool,
    exit_after_stop: bool,
    retained: Option<RetainedTake>,
    next_operation: u64,
    operation: Option<u64>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            transport: Transport::Stopped,
            exit_confirmation: false,
            exit_after_stop: false,
            retained: None,
            next_operation: 1,
            operation: None,
        }
    }
}

impl Session {
    pub fn transport(&self) -> Transport {
        self.transport
    }
    pub fn exit_confirmation(&self) -> bool {
        self.exit_confirmation
    }
    pub fn retained(&self) -> Option<&RetainedTake> {
        self.retained.as_ref()
    }

    /// Current accepted capture operation; retired completions must retain their original ID.
    pub fn operation(&self) -> Option<u64> {
        self.operation
    }

    pub fn record(&mut self) -> Result<Effect, &'static str> {
        if self.transport != Transport::Stopped {
            return Err("Stop before recording");
        }
        let operation = self.next_operation;
        self.next_operation = operation.checked_add(1).ok_or("Operation IDs exhausted")?;
        self.operation = Some(operation);
        self.retained = None;
        self.transport = Transport::Starting;
        Ok(Effect::PrepareCapture(operation))
    }

    /// The worker has opened all files and is ready to consume the ring.
    pub fn capture_ready(&mut self, operation: u64) -> Result<(), &'static str> {
        if self.operation != Some(operation) || self.transport != Transport::Starting {
            return Err("Unexpected writer readiness");
        }
        self.transport = Transport::Recording;
        Ok(())
    }

    /// Used for both startup failure and normal/faulted finalization. The adapter
    /// must first quiesce the producer and join the worker, including on failure.
    pub fn capture_finished(
        &mut self,
        operation: u64,
        take: RetainedTake,
    ) -> Result<Effect, &'static str> {
        if self.operation != Some(operation)
            || !matches!(
                self.transport,
                Transport::Starting | Transport::Recording | Transport::Finalizing
            )
        {
            return Err("Unexpected capture completion");
        }
        self.operation = None;
        let complete = take.complete;
        self.retained = Some(take);
        self.transport = Transport::Stopped;
        self.exit_confirmation = false;
        // A damaged result stays visible; the user can explicitly Exit afterward.
        if self.exit_after_stop && complete {
            return Ok(self.finish_exit());
        }
        self.exit_after_stop = false;
        Ok(Effect::None)
    }

    /// Called only after take validation, explicit route selection and prefetch.
    pub fn play(&mut self) -> Result<Effect, &'static str> {
        match self.transport {
            Transport::Stopped => {
                self.transport = Transport::Playing;
                Ok(Effect::StartPlayback)
            }
            Transport::Paused => {
                self.transport = Transport::Playing;
                Ok(Effect::ResumePlayback)
            }
            _ => Err("Stop before playback"),
        }
    }

    pub fn pause(&mut self) -> Result<Effect, &'static str> {
        if self.transport != Transport::Playing {
            return Err("Nothing is playing");
        }
        self.transport = Transport::Paused;
        Ok(Effect::PausePlayback)
    }

    pub fn stop(&mut self) -> Effect {
        self.exit_confirmation = false;
        match self.transport {
            Transport::Starting | Transport::Recording => {
                self.transport = Transport::Finalizing;
                Effect::QuiesceAndFinalize(self.operation.expect("active capture"))
            }
            Transport::Playing | Transport::Paused => {
                self.transport = Transport::StoppingPlayback;
                Effect::QuiescePlayback
            }
            _ => Effect::None,
        }
    }

    pub fn playback_stopped(&mut self) -> Result<Effect, &'static str> {
        if !matches!(
            self.transport,
            Transport::Playing | Transport::Paused | Transport::StoppingPlayback
        ) {
            return Err("Unexpected playback completion");
        }
        self.transport = Transport::Stopped;
        if self.exit_after_stop {
            Ok(self.finish_exit())
        } else {
            Ok(Effect::None)
        }
    }

    pub fn request_exit(&mut self) -> Effect {
        match self.transport {
            Transport::Starting | Transport::Recording => {
                self.exit_confirmation = true;
                Effect::None
            }
            Transport::Finalizing | Transport::StoppingPlayback => {
                self.exit_after_stop = true;
                Effect::None
            }
            Transport::Playing | Transport::Paused => {
                self.exit_after_stop = true;
                self.stop()
            }
            Transport::Stopped => self.finish_exit(),
            Transport::Exited => Effect::None,
        }
    }

    pub fn cancel_exit(&mut self) {
        self.exit_confirmation = false;
    }

    pub fn confirm_exit(&mut self) -> Effect {
        if !self.exit_confirmation {
            return Effect::None;
        }
        self.exit_after_stop = true;
        self.stop()
    }

    fn finish_exit(&mut self) -> Effect {
        self.transport = Transport::Exited;
        self.exit_confirmation = false;
        self.exit_after_stop = false;
        Effect::ReturnToCaller
    }
}
