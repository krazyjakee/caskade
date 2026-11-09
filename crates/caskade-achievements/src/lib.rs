//! Achievements (#185) and the notification feed (#198), over
//! `caskade-core`'s library store and bus. The feed announces achievement
//! unlocks, so the two live together; the core keeps what it syncs
//! (releases' achievement links) and the bus event's payload.

pub mod achievements;
pub mod notifications;
