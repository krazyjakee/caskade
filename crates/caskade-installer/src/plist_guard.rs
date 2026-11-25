//! Binary plist DAG guard: bounds depth, object count, string sizes, and
//! expanded bytes before a recursive decoder constructs a value tree.

use std::collections::BTreeSet;

/// Allocation and recursion budgets for binary plist validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_expanded_bytes: usize,
    pub max_string_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    #[error("invalid binary plist object graph")]
    Invalid,
    #[error("{budget} exceeds limit {maximum}")]
    Limit {
        budget: &'static str,
        maximum: usize,
    },
}

fn integer(data: &[u8], offset: usize, width: usize) -> Result<usize, GuardError> {
    let bytes = data
        .get(offset..offset.checked_add(width).ok_or(GuardError::Invalid)?)
        .ok_or(GuardError::Invalid)?;
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_mul(256)
            .and_then(|n| n.checked_add(usize::from(*byte)))
            .ok_or(GuardError::Invalid)?;
    }
    Ok(value)
}

fn check(actual: usize, maximum: usize, budget: &'static str) -> Result<(), GuardError> {
    if actual > maximum {
        Err(GuardError::Limit { budget, maximum })
    } else {
        Ok(())
    }
}

/// Bound both depth and expansion of a binary plist DAG before `plist::Value`'s
/// recursive tree builder is called. Offsets and references stay in the input.
pub fn validate(data: &[u8], limits: &Limits) -> Result<(), GuardError> {
    if data.len() < 40 {
        return Err(GuardError::Invalid);
    }
    let trailer = &data[data.len() - 32..];
    let width = usize::from(trailer[6]);
    let refs = usize::from(trailer[7]);
    let count = integer(trailer, 8, 8)?;
    let root = integer(trailer, 16, 8)?;
    let table = integer(trailer, 24, 8)?;
    if count == 0
        || count > limits.max_nodes
        || root >= count
        || !(1..=8).contains(&width)
        || !(1..=8).contains(&refs)
        || table < 8
        || count
            .checked_mul(width)
            .and_then(|n| table.checked_add(n))
            .is_none_or(|n| n > data.len() - 32)
    {
        return Err(GuardError::Invalid);
    }
    let mut stack = vec![(root, 0, false)];
    let mut ancestors = BTreeSet::new();
    let mut visited = 0;
    let mut expanded: usize = 0;
    while let Some((id, depth, exit)) = stack.pop() {
        if exit {
            ancestors.remove(&id);
            continue;
        }
        visited += 1;
        if visited > limits.max_nodes || depth > limits.max_depth.min(128) {
            return Err(GuardError::Limit {
                budget: "plist graph",
                maximum: limits.max_nodes,
            });
        }
        if id >= count || !ancestors.insert(id) {
            return Err(GuardError::Invalid);
        }
        let offset = integer(data, table + id * width, width)?;
        if !(8..table).contains(&offset) {
            return Err(GuardError::Invalid);
        }
        let marker = data[offset];
        let ty = marker >> 4;
        let mut length = usize::from(marker & 15);
        let mut start = offset + 1;
        if matches!(ty, 4 | 5 | 6 | 10 | 13) && length == 15 {
            let marker = *data.get(start).ok_or(GuardError::Invalid)?;
            if marker >> 4 != 1 || marker & 15 > 3 {
                return Err(GuardError::Invalid);
            }
            let size = 1_usize << (marker & 15);
            length = integer(data, start + 1, size)?;
            start += 1 + size;
        }
        if matches!(ty, 4 | 5 | 6 | 10 | 13) {
            let units = match ty {
                6 => 2,
                10 => refs,
                13 => refs * 2,
                _ => 1,
            };
            if length
                .checked_mul(units)
                .and_then(|n| start.checked_add(n))
                .is_none_or(|n| n > table)
            {
                return Err(GuardError::Invalid);
            }
        }
        if matches!(ty, 4..=6) {
            let bytes = length.saturating_mul(if ty == 6 { 3 } else { 1 });
            if ty != 4 {
                check(bytes, limits.max_string_bytes, "string bytes")?;
            }
            expanded = expanded.saturating_add(bytes);
            check(expanded, limits.max_expanded_bytes, "expanded plist bytes")?;
        }
        if matches!(ty, 10 | 13) {
            if length > limits.max_nodes {
                return Err(GuardError::Limit {
                    budget: "plist graph",
                    maximum: limits.max_nodes,
                });
            }
            let children = length * if ty == 13 { 2 } else { 1 };
            check(
                stack.len().saturating_add(children),
                limits.max_nodes,
                "plist graph",
            )?;
            stack.push((id, depth, true));
            for index in (0..children).rev() {
                stack.push((integer(data, start + index * refs, refs)?, depth + 1, false));
            }
        } else {
            ancestors.remove(&id);
        }
    }
    Ok(())
}
