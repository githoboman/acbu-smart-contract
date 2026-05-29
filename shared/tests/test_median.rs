#![cfg(test)]

use shared::median;
use soroban_sdk::{Env, Vec};

fn make_vec(env: &Env, values: &[i128]) -> Vec<i128> {
    let mut v = Vec::new(env);
    for &x in values {
        v.push_back(x);
    }
    v
}

#[test]
fn test_median_empty_returns_none() {
    let env = Env::default();
    let v: Vec<i128> = Vec::new(&env);
    assert_eq!(median(v), None);
}

#[test]
fn test_median_single_element() {
    let env = Env::default();
    let v = make_vec(&env, &[42]);
    assert_eq!(median(v), Some(42));
}

#[test]
fn test_median_odd_count_sorted() {
    let env = Env::default();
    let v = make_vec(&env, &[1, 3, 5]);
    assert_eq!(median(v), Some(3));
}

#[test]
fn test_median_odd_count_unsorted() {
    let env = Env::default();
    let v = make_vec(&env, &[5, 1, 3]);
    assert_eq!(median(v), Some(3));
}

#[test]
fn test_median_five_elements_unsorted() {
    let env = Env::default();
    // sorted: [1, 2, 3, 4, 5] → median = 3
    let v = make_vec(&env, &[3, 1, 4, 2, 5]);
    assert_eq!(median(v), Some(3));
}

#[test]
fn test_median_even_count_sorted() {
    let env = Env::default();
    // sorted: [1, 3, 5, 7] → median = (3 + 5) / 2 = 4
    let v = make_vec(&env, &[1, 3, 5, 7]);
    assert_eq!(median(v), Some(4));
}

#[test]
fn test_median_even_count_unsorted() {
    let env = Env::default();
    let v = make_vec(&env, &[7, 1, 5, 3]);
    assert_eq!(median(v), Some(4));
}

#[test]
fn test_median_two_elements() {
    let env = Env::default();
    let v = make_vec(&env, &[10, 20]);
    assert_eq!(median(v), Some(15));
}

#[test]
fn test_median_all_equal() {
    let env = Env::default();
    let v = make_vec(&env, &[7, 7, 7, 7, 7]);
    assert_eq!(median(v), Some(7));
}

#[test]
fn test_median_large_7decimal_rates() {
    let env = Env::default();
    // Simulate oracle source rates at 7 decimals
    let v = make_vec(&env, &[1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000]);
    assert_eq!(median(v), Some(3_000_000));
}

#[test]
fn test_median_six_elements_even_count() {
    let env = Env::default();
    // sorted: [1, 2, 3, 4, 5, 6] → (3 + 4) / 2 = 3
    let v = make_vec(&env, &[4, 1, 6, 3, 2, 5]);
    assert_eq!(median(v), Some(3));
}

#[test]
fn test_median_reverse_sorted() {
    let env = Env::default();
    let v = make_vec(&env, &[9, 7, 5, 3, 1]);
    assert_eq!(median(v), Some(5));
}
