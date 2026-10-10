//! Order venues other than Binance, through the official ccxt crate.

// ccxt reports every error as a panic that it catches (`catch_unwind`). With
// `panic = "abort"` each exchange error would kill the app instead.
#[cfg(panic = "abort")]
compile_error!("the ccxt venues need panic = \"unwind\"");

pub mod ccxt;

#[cfg(test)]
mod ccxt_spike_tests;
