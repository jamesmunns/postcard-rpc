//! Accumulator tools
//!
//! These tools are useful for accumulating and decoding COBS encoded messages.
//!
//! Unlike the `CobsAccumulator` from `postcard`, these versions do not deserialize
//! directly.

/// Decode-only accumulator
pub mod raw {
    use cobs::decode_in_place;

    pub struct CobsAccumulator<const N: usize> {
        buf: [u8; N],
        idx: usize,
    }

    /// The result of feeding the accumulator.
    #[cfg_attr(feature = "defmt-03", derive(defmt::Format))]
    pub enum FeedResult<'a, 'b> {
        /// Consumed all data, still pending.
        Consumed,

        /// Buffer was filled. Contains remaining section of input, if any.
        OverFull(&'a [u8]),

        /// Reached end of chunk, but deserialization failed. Contains remaining section of input, if.
        /// any
        DeserError(&'a [u8]),

        /// Deserialization complete. Contains deserialized data and remaining section of input, if any.
        Success {
            /// Deserialize data.
            data: &'b [u8],

            /// Remaining data left in the buffer after deserializing.
            remaining: &'a [u8],
        },
    }

    impl<const N: usize> CobsAccumulator<N> {
        /// Create a new accumulator.
        pub const fn new() -> Self {
            CobsAccumulator {
                buf: [0; N],
                idx: 0,
            }
        }

        /// Appends data to the internal buffer and attempts to deserialize the accumulated data into
        /// `T`.
        #[inline]
        pub fn feed<'a, 'b>(&'b mut self, input: &'a [u8]) -> FeedResult<'a, 'b> {
            self.feed_ref(input)
        }

        /// Appends data to the internal buffer and attempts to deserialize the accumulated data into
        /// `T`.
        ///
        /// This differs from feed, as it allows the `T` to reference data within the internal buffer, but
        /// mutably borrows the accumulator for the lifetime of the deserialization.
        /// If `T` does not require the reference, the borrow of `self` ends at the end of the function.
        pub fn feed_ref<'a, 'b>(&'b mut self, input: &'a [u8]) -> FeedResult<'a, 'b> {
            if input.is_empty() {
                return FeedResult::Consumed;
            }

            let zero_pos = input.iter().position(|&i| i == 0);

            if let Some(n) = zero_pos {
                // Yes! We have an end of message here.
                // Add one to include the zero in the "take" portion
                // of the buffer, rather than in "release".
                let (take, release) = input.split_at(n + 1);

                // Does it fit?
                if (self.idx + take.len()) <= N {
                    // Aw yiss - add to array
                    self.extend_unchecked(take);

                    let retval = match decode_in_place(&mut self.buf[..self.idx]) {
                        Ok(used) => FeedResult::Success {
                            data: &self.buf[..used],
                            remaining: release,
                        },
                        Err(_) => FeedResult::DeserError(release),
                    };
                    self.idx = 0;
                    retval
                } else {
                    self.idx = 0;
                    FeedResult::OverFull(release)
                }
            } else {
                // Does it fit?
                if (self.idx + input.len()) > N {
                    // nope
                    let new_start = N - self.idx;
                    self.idx = 0;
                    FeedResult::OverFull(&input[new_start..])
                } else {
                    // yup!
                    self.extend_unchecked(input);
                    FeedResult::Consumed
                }
            }
        }

        /// Extend the internal buffer with the given input.
        ///
        /// # Panics
        ///
        /// Will panic if the input does not fit in the internal buffer.
        fn extend_unchecked(&mut self, input: &[u8]) {
            let new_end = self.idx + input.len();
            self.buf[self.idx..new_end].copy_from_slice(input);
            self.idx = new_end;
        }
    }
}

/// Accumulate and Dispatch
pub mod dispatch {
    use super::raw::{CobsAccumulator, FeedResult};
    use crate::Dispatch;

    /// An error containing the handler-specific error, as well as the unprocessed
    /// feed bytes
    #[derive(Debug, PartialEq)]
    pub struct FeedError<'a, E> {
        pub err: E,
        pub remainder: &'a [u8],
    }

    /// A COBS-flavored version of [Dispatch]
    ///
    /// [Dispatch]: crate::Dispatch
    ///
    /// CobsDispatch is generic over four types:
    ///
    /// 1. The `Context`, which will be passed as a mutable reference
    ///    to each of the handlers. It typically should contain
    ///    whatever resource is necessary to send replies back to
    ///    the host.
    /// 2. The `Error` type, which can be returned by handlers
    /// 3. `N`, for the maximum number of handlers
    /// 4. `BUF`, for the maximum number of bytes to buffer for a single
    ///    COBS-encoded message
    pub struct CobsDispatch<Context, Error, const N: usize, const BUF: usize> {
        dispatch: Dispatch<Context, Error, N>,
        acc: CobsAccumulator<BUF>,
    }

    impl<Context, Error, const N: usize, const BUF: usize> CobsDispatch<Context, Error, N, BUF> {
        /// Create a new `CobsDispatch`
        pub fn new(c: Context) -> Self {
            Self {
                dispatch: Dispatch::new(c),
                acc: CobsAccumulator::new(),
            }
        }

        /// Access the contained [Dispatch]`
        pub fn dispatcher(&mut self) -> &mut Dispatch<Context, Error, N> {
            &mut self.dispatch
        }

        /// Feed the given bytes into the dispatcher, attempting to dispatch each framed
        /// message found.
        ///
        /// Line format errors, such as an overfull buffer or bad COBS frames will be
        /// silently ignored.
        ///
        /// If an error in dispatching occurs, this function will return immediately,
        /// yielding the error and the remaining unprocessed bytes for further processing.
        pub fn feed<'a>(
            &mut self,
            buf: &'a [u8],
        ) -> Result<(), FeedError<'a, crate::Error<Error>>> {
            let mut window = buf;
            let CobsDispatch { dispatch, acc } = self;
            'cobs: while !window.is_empty() {
                window = match acc.feed(window) {
                    FeedResult::Consumed => break 'cobs,
                    FeedResult::OverFull(new_wind) => new_wind,
                    FeedResult::DeserError(new_wind) => new_wind,
                    FeedResult::Success { data, remaining } => {
                        dispatch.dispatch(data).map_err(|e| FeedError {
                            err: e,
                            remainder: remaining,
                        })?;
                        remaining
                    }
                };
            }

            // We have dispatched all (if any) messages, and consumed the buffer
            // without dispatch errors.
            Ok(())
        }

        /// Similar to [CobsDispatch::feed], but the provided closure is called on each
        /// error, allowing for handling.
        ///
        /// Useful if you need to do something blocking on each error case.
        ///
        /// If you need to handle the error in an async context, you may want to use
        /// [CobsDispatch::feed] instead.
        pub fn feed_with_err<F>(&mut self, buf: &[u8], mut f: F)
        where
            F: FnMut(&mut Context, crate::Error<Error>),
        {
            let mut window = buf;
            while let Err(FeedError { err, remainder }) = self.feed(window) {
                f(&mut self.dispatch.context, err);
                window = remainder;
            }
        }
    }
}
