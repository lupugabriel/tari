// LWMA-1 for BTC & Zcash clones
// Copyright (c) 2017-2019 The Bitcoin Gold developers, Zawy, iamstenman (Microbitcoin)
// MIT License
// Algorithm by Zawy, a modification of WT-144 by Tom Harding
// References:
// https://github.com/zawy12/difficulty-algorithms/issues/3#issuecomment-442129791
// https://github.com/zcash/zcash/issues/4021

use crate::{
    consensus::ConsensusConstants,
    proof_of_work::{
        difficulty::{Difficulty, DifficultyAdjustment},
        error::DifficultyAdjustmentError,
    },
};
use std::{cmp, collections::VecDeque};
use tari_utilities::epoch_time::EpochTime;

const INITIAL_DIFFICULTY: Difficulty = Difficulty::min();

pub struct LinearWeightedMovingAverage {
    timestamps: VecDeque<EpochTime>,
    accumulated_difficulties: VecDeque<Difficulty>,
    block_window: usize,
    target_time: u64,
}

impl Default for LinearWeightedMovingAverage {
    fn default() -> Self {
        let consensus = ConsensusConstants::current();
        LinearWeightedMovingAverage::new(
            consensus.get_difficulty_block_window() as usize,
            consensus.get_diff_target_block_interval(),
        )
    }
}

impl LinearWeightedMovingAverage {
    pub fn new(block_window: usize, target_time: u64) -> LinearWeightedMovingAverage {
        LinearWeightedMovingAverage {
            timestamps: VecDeque::with_capacity(block_window + 1),
            accumulated_difficulties: VecDeque::with_capacity(block_window + 1),
            block_window,
            target_time,
        }
    }

    fn calculate(&self) -> Difficulty {
        let timestamps = &self.timestamps;
        if timestamps.len() <= 1 {
            return INITIAL_DIFFICULTY;
        }

        // Use the array length rather than block_window to include early cases where the no. of pts < block_window
        let n = (timestamps.len() - 1) as u64;

        let mut weighted_times: u64 = 0;

        let difficulty: u64 = self.accumulated_difficulties[n as usize]
            .checked_sub(self.accumulated_difficulties[0])
            .expect("Accumulated difficulties cannot decrease in proof of work")
            .as_u64();
        let ave_difficulty = difficulty as f64 / n as f64;

        let mut previous_timestamp = timestamps[0];
        let mut this_timestamp;
        // Loop through N most recent blocks.
        for i in 1..(n + 1) as usize {
            // 6*T limit prevents large drops in diff from long solve times which would cause oscillations.
            // We cannot have if solve_time < 1 then solve_time = 1, this will greatly increase the next timestamp
            // difficulty which will lower the difficulty
            if timestamps[i] > previous_timestamp {
                this_timestamp = timestamps[i];
            } else {
                this_timestamp = previous_timestamp.increase(1);
            }
            let solve_time = cmp::min((this_timestamp - previous_timestamp).as_u64(), 6 * self.target_time);
            previous_timestamp = this_timestamp;

            // Give linearly higher weight to more recent solve times.
            // Note: This will not overflow for practical values of block_window and solve time.
            weighted_times += solve_time * i as u64;
        }
        // k is the sum of weights (1+2+..+n) * target_time
        let k = n * (n + 1) * self.target_time / 2;
        let target = ave_difficulty * k as f64 / weighted_times as f64;
        if target > std::u64::MAX as f64 {
            panic!("Difficulty target has overflowed");
        }
        let target = target.ceil() as u64; // difficulty difference of 1 should not matter much, but difficulty should never be below 1, ceil(0.9) = 1
        target.into()
    }
}

impl DifficultyAdjustment for LinearWeightedMovingAverage {
    fn add(
        &mut self,
        timestamp: EpochTime,
        accumulated_difficulty: Difficulty,
    ) -> Result<(), DifficultyAdjustmentError>
    {
        match self.accumulated_difficulties.back() {
            None => {},
            Some(v) => {
                if accumulated_difficulty <= *v {
                    return Err(DifficultyAdjustmentError::DecreasingAccumulatedDifficulty);
                }
            },
        };
        self.timestamps.push_back(timestamp);
        self.accumulated_difficulties.push_back(accumulated_difficulty);
        while self.timestamps.len() > self.block_window + 1 {
            self.timestamps.pop_front();
            self.accumulated_difficulties.pop_front();
        }
        Ok(())
    }

    fn get_difficulty(&self) -> Difficulty {
        self.calculate()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn lwma_zero_len() {
        let dif = LinearWeightedMovingAverage::default();
        assert_eq!(dif.get_difficulty(), Difficulty::min());
    }

    #[test]
    fn lwma_add_non_increasing_diff() {
        let mut dif = LinearWeightedMovingAverage::default();
        assert!(dif.add(100.into(), 100.into()).is_ok());
        assert!(dif.add(100.into(), 100.into()).is_err());
        assert!(dif.add(100.into(), 50.into()).is_err());
    }

    #[test]
    fn lwma_negative_solve_times() {
        let mut dif = LinearWeightedMovingAverage::default();
        let mut timestamp = 60.into();
        let mut cum_diff = Difficulty::from(100);
        let _ = dif.add(timestamp, cum_diff);
        timestamp = timestamp.increase(60);
        cum_diff += Difficulty::from(100);
        let _ = dif.add(timestamp, cum_diff);
        // Lets create a history and populate the vecs
        for _i in 0..150 {
            cum_diff += Difficulty::from(100);
            timestamp = timestamp.increase(60);
            let _ = dif.add(timestamp, cum_diff);
        }
        // lets create chaos by having 60 blocks as negative solve times. This should never be allowed in practice by
        // having checks on the block times.
        for _i in 0..60 {
            cum_diff += Difficulty::from(100);
            timestamp = (timestamp.as_u64() - 1).into(); // Only choosing -1 here since we are testing negative solve times and we cannot have 0 time
            let diff_before = dif.get_difficulty();
            let _ = dif.add(timestamp, cum_diff);
            let diff_after = dif.get_difficulty();
            // Algo should handle this as 1sec solve time thus increase the difficulty constantly
            assert!(diff_after > diff_before);
        }
    }

    #[test]
    fn lwma_limit_difficulty_change() {
        let mut dif = LinearWeightedMovingAverage::new(5, 60);
        let _ = dif.add(60.into(), 100.into());
        let _ = dif.add(10_000_000.into(), 200.into());
        assert_eq!(dif.get_difficulty(), 17.into());
        let _ = dif.add(20_000_000.into(), 216.into());
        assert_eq!(dif.get_difficulty(), 10.into());
    }

    #[test]
    // Data for 5-period moving average
    // Timestamp: 60, 120, 180, 240, 300, 350, 380, 445, 515, 615, 975, 976, 977, 978, 979
    // Intervals: 60,  60,  60,  60,  60,  50,  30,  65,  70, 100, 360,   1,   1,   1,   1
    // Diff:     100, 100, 100, 100, 100, 105, 128, 123, 116,  94,  39,  46,  55,  75, 148
    // Acum dif: 100, 200, 300, 400, 500, 605, 733, 856, 972,1066,1105,1151,1206,1281,1429
    // Target:     1, 100, 100, 100, 100, 107, 136, 130, 120,  94,  36,  39,  47,  67, 175
    fn lwma_calculate() {
        let mut dif = LinearWeightedMovingAverage::new(5, 60);
        let _ = dif.add(60.into(), 100.into());
        assert_eq!(dif.get_difficulty(), 1.into());
        let _ = dif.add(120.into(), 200.into());
        assert_eq!(dif.get_difficulty(), 100.into());
        let _ = dif.add(180.into(), 300.into());
        assert_eq!(dif.get_difficulty(), 100.into());
        let _ = dif.add(240.into(), 400.into());
        assert_eq!(dif.get_difficulty(), 100.into());
        let _ = dif.add(300.into(), 500.into());
        assert_eq!(dif.get_difficulty(), 100.into());
        let _ = dif.add(350.into(), 605.into());
        assert_eq!(dif.get_difficulty(), 107.into());
        let _ = dif.add(380.into(), 733.into());
        assert_eq!(dif.get_difficulty(), 136.into());
        let _ = dif.add(445.into(), 856.into());
        assert_eq!(dif.get_difficulty(), 130.into());
        let _ = dif.add(515.into(), 972.into());
        assert_eq!(dif.get_difficulty(), 120.into());
        let _ = dif.add(615.into(), 1066.into());
        assert_eq!(dif.get_difficulty(), 94.into());
        let _ = dif.add(975.into(), 1105.into());
        assert_eq!(dif.get_difficulty(), 36.into());
        let _ = dif.add(976.into(), 1151.into());
        assert_eq!(dif.get_difficulty(), 39.into());
        let _ = dif.add(977.into(), 1206.into());
        assert_eq!(dif.get_difficulty(), 47.into());
        let _ = dif.add(978.into(), 1281.into());
        assert_eq!(dif.get_difficulty(), 67.into());
        let _ = dif.add(979.into(), 1429.into());
        assert_eq!(dif.get_difficulty(), 175.into());
    }
}
