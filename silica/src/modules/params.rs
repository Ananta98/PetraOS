// SPDX-License-Identifier: GPL-2.0

//! Parsing of Linux-style module parameters (`key=value` lists).

use alloc::{borrow::ToOwned, string::String, vec::Vec};

/// Parses a module parameter string such as `debug=1,option="foo bar"`.
///
/// Keys and values are trimmed; surrounding single or double quotes on
/// values are stripped. Keys without `=` get an empty value, mirroring
/// `insmod` behavior.
pub fn parse_modparams(cmdline: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    for item in cmdline.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (key, value) = match item.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (item, ""),
        };
        let value = value.trim_matches('"').trim_matches('\'').to_owned();
        params.push((key.to_owned(), value));
    }
    params
}
