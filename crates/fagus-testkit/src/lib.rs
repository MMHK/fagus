//! Conformance tests for Fagus: the reftests of web-platform-tests rendered by Fagus on both
//! sides ([`wpt`]), and the corpus against Chrome ([`corpus`]). `tools/wpt/run.sh` and
//! `tools/chrome-corpus.sh` run them through the binary `fagus-conformance`.

pub mod corpus;
pub mod process;
pub mod raster;
pub mod wpt;
