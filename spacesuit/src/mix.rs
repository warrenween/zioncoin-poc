#![allow(non_snake_case)]

use crate::signed_integer::SignedInteger;
use crate::value::{AllocatedValue, Value};
use bulletproofs::r1cs::{
    ConstraintSystem, R1CSError, RandomizableConstraintSystem, RandomizedConstraintSystem,
};
use curve25519_dalek::scalar::Scalar;
use std::iter;
use subtle::{ConditionallySelectable, ConstantTimeEq};

/// Enforces that the outputs are either a merge of the inputs :`D = A + B && C = 0`,
/// or the outputs are equal to the inputs `C = A && D = B`. See spec for more details.
/// Works for 2 inputs and 2 outputs.
pub fn mix<CS: RandomizableConstraintSystem>(
    cs: &mut CS,
    A: AllocatedValue,
    B: AllocatedValue,
    C: AllocatedValue,
    D: AllocatedValue,
) -> Result<(), R1CSError> {
    cs.specify_randomized_constraints(move |cs| {
        let w = cs.challenge_scalar(b"mix challenge");
        let w2 = w * w;
        let w3 = w2 * w;

        let (_, _, mul_out) = cs.multiply(
            (A.q - C.q) + (A.f - C.f) * w + (B.q - D.q) * w2 + (B.f - D.f) * w3,
            C.q + (A.f - B.f) * w + (D.q - A.q - B.q) * w2 + (D.f - A.f) * w3,
        );

        // multiplication output is zero
        cs.constrain(mul_out.into());

        Ok(())
    })
}

/// Takes:
/// * a vector of `k` input `AllocatedValue`s provided in arbitrary order.
///
/// Returns:
/// * a vector of `k` sorted `AllocatedValue`s that are the inputs to the `mix` gadget
/// * a vector of `k` `AllocatedValue`s that are the outputs to the `mix` gadget,
///   such that each output is either zero, or the sum of all of the `Values` of one type.
pub fn k_mix<CS: RandomizableConstraintSystem>(
    cs: &mut CS,
    inputs: Vec<AllocatedValue>,
) -> Result<(Vec<AllocatedValue>, Vec<AllocatedValue>), R1CSError> {
    // If there is only one input and output, simply reuse the input wires as output wires.
    if inputs.len() == 1 {
        return Ok((inputs.clone(), inputs));
    }

    let (mix_in, mix_mid, mix_out) = make_intermediate_values(&inputs, cs)?;
    call_mix_gadget(cs, &mix_in, &mix_mid, &mix_out)?;
    Ok((mix_in, mix_out))
}

// Calls `k` mix gadgets, using mix_in and mix_mid as inputs, and mix_mid and mix_out as outputs.
fn call_mix_gadget<CS: RandomizableConstraintSystem>(
    cs: &mut CS,
    mix_in: &Vec<AllocatedValue>,
    mix_mid: &Vec<AllocatedValue>,
    mix_out: &Vec<AllocatedValue>,
) -> Result<(), R1CSError> {
    let k = mix_out.len();
    if mix_in.len() != k || mix_mid.len() != k - 2 {
        return Err(R1CSError::GadgetError {
            description: "Lengths of inputs are incorrect for call_mix_gadget in k_mix".to_string(),
        });
    }

    // The first value of mix_in, to prepend to mix_mid for creating A inputs.
    let first_in = mix_in[0].clone();
    // The last value of mix_out, to append to mix_mid for creating D outputs.
    let last_out = mix_out[k - 1].clone();

    // For each of the `k-1` mix gadget calls, constrain A, B, C, D:
    for (((A, B), C), D) in
        // A = (first_in||mix_mid)[i]
        iter::once(&first_in).chain(mix_mid.iter())
        // B = mix_in[i+1]
        .zip(mix_in.iter().skip(1))
        // C = mix_out[i]
        .zip(mix_out.iter().take(k-1))
        // D = (mix_mid||last_out)[i]
        .zip(mix_mid.iter().chain(iter::once(&last_out)))
    {
        mix(cs, *A, *B, *C, *D)?
    }

    Ok(())
}

// Takes:
// * a vector of `AllocatedValue`s that represents the input (or output) values for a cloak gadget
//
// Returns:
// * a vector of `AllocatedValue`s for the input values of a k-mix gadget
// * a vector of `AllocatedValue`s for the middle values of a k-mix gadget
// * a vector of `AllocatedValue`s for the output values of a k-mix gadget
fn make_intermediate_values<CS: RandomizableConstraintSystem>(
    inputs: &Vec<AllocatedValue>,
    cs: &mut CS,
) -> Result<
    (
        Vec<AllocatedValue>,
        Vec<AllocatedValue>,
        Vec<AllocatedValue>,
    ),
    R1CSError,
> {
    let collected_inputs: Option<Vec<_>> = inputs.iter().map(|input| input.assignment).collect();
    match collected_inputs {
        Some(input_values) => {
            let (mix_in, mix_in_values) = order_by_flavor(&input_values, cs)?;
            let (mix_mid, mix_out) = combine_by_flavor(&mix_in_values, cs)?;
            Ok((mix_in, mix_mid, mix_out))
        }
        None => {
            let mix_in = AllocatedValue::unassigned_vec(cs, inputs.len())?;
            let mix_mid = AllocatedValue::unassigned_vec(cs, inputs.len() - 2)?;
            let mix_out = AllocatedValue::unassigned_vec(cs, inputs.len())?;
            Ok((mix_in, mix_mid, mix_out))
        }
    }
}

// Takes:
// * a vector of `AllocatedValue`s
//
// Returns:
// * a vector of `AllocatedValue`s that is a reordering of the inputs
//   where all `AllocatedValues` have been grouped according to flavor
// * a vector of `Value`s that were used to create the output `AllocatedValue`s
fn order_by_flavor<CS: RandomizableConstraintSystem>(
    inputs: &Vec<Value>,
    cs: &mut CS,
) -> Result<(Vec<AllocatedValue>, Vec<Value>), R1CSError> {
    let k = inputs.len();
    let mut outputs = inputs.clone();

    for i in 0..k - 1 {
        // This tuple has the flavor that we are trying to group by in this loop
        let flav = outputs[i];
        // This tuple may be swapped with another tuple (`comp`)
        // if `comp` and `flav` have the same flavor.
        let mut swap = outputs[i + 1];

        for j in i + 2..k {
            // Iterate over all following tuples, assigning them to `comp`.
            let mut comp = outputs[j];
            // Check if `flav` and `comp` have the same flavor.
            let same_flavor = flav.f.ct_eq(&comp.f);

            // If same_flavor, then swap `comp` and `swap`. Else, keep the same.
            SignedInteger::conditional_swap(&mut swap.q, &mut comp.q, same_flavor);
            Scalar::conditional_swap(&mut swap.f, &mut comp.f, same_flavor);
            outputs[i + 1] = swap;
            outputs[j] = comp;
        }
    }

    let allocated_outputs = outputs
        .iter()
        .map(|value| value.allocate(cs))
        .collect::<Result<Vec<AllocatedValue>, _>>()?;

    Ok((allocated_outputs, outputs))
}

// Takes:
// * a vector of `Value`s that are grouped according to flavor
//
// Returns:
// * a vector of the `AllocatedValue`s that are both outputs and inputs to 2-mix gadgets,
//   where `Value`s of the same flavor are combined and `Value`s of different flavors
//   are moved without modification. (See `mix.rs` for more information on 2-mix gadgets.)
// * a vector of the `AllocatedValue`s that are only outputs of 2-mix gadgets.
fn combine_by_flavor<CS: RandomizableConstraintSystem>(
    inputs: &Vec<Value>,
    cs: &mut CS,
) -> Result<(Vec<AllocatedValue>, Vec<AllocatedValue>), R1CSError> {
    let mut mid = Vec::with_capacity(inputs.len() - 1);
    let mut outputs = Vec::with_capacity(inputs.len());

    let mut A = inputs[0];
    for B in inputs.into_iter().skip(1) {
        // Check if A and B have the same flavors
        let same_flavor = A.f.ct_eq(&B.f);

        // If same_flavor, merge: C.0, C.1, C.2 = 0.
        // Else, move: C = A.
        let mut C = A.clone();
        C.q.conditional_assign(&0u64.into(), same_flavor);
        C.f.conditional_assign(&Scalar::zero(), same_flavor);
        outputs.push(C);

        // If same_flavor, merge: D.0 = A.0 + B.0, D.1 = A.1, D.2 = A.2.
        // Else, move: D = B.
        let mut D = B.clone();
        match A.q + B.q {
            Some(x) => D.q.conditional_assign(&x, same_flavor),
            None => {
                return Err(R1CSError::GadgetError {
                    description: "Overflow adding quantities".to_string(),
                });
            }
        };
        D.f.conditional_assign(&A.f, same_flavor);
        mid.push(D);

        A = D;
    }

    // Move the last mid to be the last output, to match the protocol definition
    match mid.pop() {
        Some(val) => outputs.push(val),
        None => {
            return Err(R1CSError::GadgetError {
                description: "Last merge_mid was not popped successfully in combine_by_flavor"
                    .to_string(),
            });
        }
    }

    let allocated_mid = mid
        .iter()
        .map(|value| value.allocate(cs))
        .collect::<Result<Vec<AllocatedValue>, _>>()?;
    let allocated_outputs = outputs
        .iter()
        .map(|value| value.allocate(cs))
        .collect::<Result<Vec<AllocatedValue>, _>>()?;

    Ok((allocated_mid, allocated_outputs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bulletproofs::r1cs::{Prover, Verifier};
    use bulletproofs::{BulletproofGens, PedersenGens};
    use merlin::Transcript;
    use value::{ProverCommittable, Value, VerifierCommittable};

    // Helper functions to make the tests easier to read
    fn yuan(q: u64) -> Value {
        Value {
            q: q.into(),
            f: 888u64.into(),
        }
    }
    fn peso(q: u64) -> Value {
        Value {
            q: q.into(),
            f: 666u64.into(),
        }
    }
    fn zero() -> Value {
        Value::zero()
    }

    #[test]
    fn test_2x2_mix() {
        let peso = 66;
        let yuan = 88;

        // no merge, same asset types
        assert!(mix_helper((6, peso), (6, peso), (6, peso), (6, peso),).is_ok());
        // no merge, different asset types
        assert!(mix_helper((3, peso), (6, yuan), (3, peso), (6, yuan),).is_ok());
        // merge, same asset types
        assert!(mix_helper((3, peso), (6, peso), (0, peso), (9, peso),).is_ok());
        // merge, zero value is different asset type
        assert!(mix_helper((3, peso), (6, peso), (0, yuan), (9, peso),).is_ok());
        // error when merging different asset types
        assert!(mix_helper((3, peso), (3, yuan), (0, peso), (6, yuan),).is_err());
        // error when not merging, but asset type changes
        assert!(mix_helper((3, peso), (3, yuan), (3, peso), (3, peso),).is_err());
        // error when creating more value (same asset types)
        assert!(mix_helper((3, peso), (3, peso), (3, peso), (6, peso),).is_err());
        // error when creating more value (different asset types)
        assert!(mix_helper((3, peso), (3, yuan), (3, peso), (6, yuan),).is_err());
    }

    fn mix_helper(
        A: (u64, u64),
        B: (u64, u64),
        C: (u64, u64),
        D: (u64, u64),
    ) -> Result<(), R1CSError> {
        // Common
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(128, 1);

        let A = Value {
            q: A.0.into(),
            f: A.1.into(),
        };
        let B = Value {
            q: B.0.into(),
            f: B.1.into(),
        };
        let C = Value {
            q: C.0.into(),
            f: C.1.into(),
        };
        let D = Value {
            q: D.0.into(),
            f: D.1.into(),
        };

        // Prover's scope
        let (proof, A_com, B_com, C_com, D_com) = {
            // Prover makes a `ConstraintSystem` instance representing a merge gadget
            let mut prover_transcript = Transcript::new(b"MixTest");
            let mut rng = rand::thread_rng();

            let mut prover = Prover::new(&pc_gens, &mut prover_transcript);
            let (A_com, A_var) = A.commit(&mut prover, &mut rng);
            let (B_com, B_var) = B.commit(&mut prover, &mut rng);
            let (C_com, C_var) = C.commit(&mut prover, &mut rng);
            let (D_com, D_var) = D.commit(&mut prover, &mut rng);

            mix(&mut prover, A_var, B_var, C_var, D_var)?;

            let proof = prover.prove(&bp_gens)?;
            (proof, A_com, B_com, C_com, D_com)
        };

        // Verifier makes a `ConstraintSystem` instance representing a merge gadget
        let mut verifier_transcript = Transcript::new(b"MixTest");
        let mut verifier = Verifier::new(&mut verifier_transcript);

        let A_var = A_com.commit(&mut verifier);
        let B_var = B_com.commit(&mut verifier);
        let C_var = C_com.commit(&mut verifier);
        let D_var = D_com.commit(&mut verifier);

        mix(&mut verifier, A_var, B_var, C_var, D_var)?;

        Ok(verifier.verify(&proof, &pc_gens, &bp_gens)?)
    }

    #[test]
    fn test_k_mix() {
        // k=2. More extensive k=2 tests are in the MixGadget tests
        // no merge, different asset types
        assert!(k_mix_helper(vec![peso(3), yuan(6)], vec![], vec![peso(3), yuan(6)],).is_ok());
        // merge, same asset types
        assert!(k_mix_helper(vec![peso(3), peso(6)], vec![], vec![peso(0), peso(9)],).is_ok());
        // error when merging different asset types
        assert!(k_mix_helper(vec![peso(3), yuan(3)], vec![], vec![peso(0), yuan(6)],).is_err());

        // k=3
        // no merge, same asset types
        assert!(k_mix_helper(
            vec![peso(3), peso(6), peso(6)],
            vec![peso(6)],
            vec![peso(3), peso(6), peso(6)],
        )
        .is_ok());
        // no merge, different asset types
        assert!(k_mix_helper(
            vec![peso(3), yuan(6), peso(6)],
            vec![yuan(6)],
            vec![peso(3), yuan(6), peso(6)],
        )
        .is_ok());
        // merge first two
        assert!(k_mix_helper(
            vec![peso(3), peso(6), yuan(1)],
            vec![peso(9)],
            vec![peso(0), peso(9), yuan(1)],
        )
        .is_ok());
        // merge last two
        assert!(k_mix_helper(
            vec![yuan(1), peso(3), peso(6)],
            vec![peso(3)],
            vec![yuan(1), peso(0), peso(9)],
        )
        .is_ok());
        // merge all, same asset types, zero value is different asset type
        assert!(k_mix_helper(
            vec![peso(3), peso(6), peso(1)],
            vec![peso(9)],
            vec![zero(), zero(), peso(10)],
        )
        .is_ok());
        // incomplete merge, input sum does not equal output sum
        assert!(k_mix_helper(
            vec![peso(3), peso(6), peso(1)],
            vec![peso(9)],
            vec![zero(), zero(), peso(9)],
        )
        .is_err());
        // error when merging with different asset types
        assert!(k_mix_helper(
            vec![peso(3), yuan(6), peso(1)],
            vec![peso(9)],
            vec![zero(), zero(), peso(10)],
        )
        .is_err());

        // k=4
        // merge each of 2 asset types
        assert!(k_mix_helper(
            vec![peso(3), peso(6), yuan(1), yuan(2)],
            vec![peso(9), yuan(1)],
            vec![zero(), peso(9), zero(), yuan(3)],
        )
        .is_ok());
        // merge all, same asset
        assert!(k_mix_helper(
            vec![peso(3), peso(2), peso(2), peso(1)],
            vec![peso(5), peso(7)],
            vec![zero(), zero(), zero(), peso(8)],
        )
        .is_ok());
        // no merge, different assets
        assert!(k_mix_helper(
            vec![peso(3), yuan(2), peso(2), yuan(1)],
            vec![yuan(2), peso(2)],
            vec![peso(3), yuan(2), peso(2), yuan(1)],
        )
        .is_ok());
        // error when merging, output sum not equal to input sum
        assert!(k_mix_helper(
            vec![peso(3), peso(2), peso(2), peso(1)],
            vec![peso(5), peso(7)],
            vec![zero(), zero(), zero(), peso(9)],
        )
        .is_err());
    }

    fn k_mix_helper(
        inputs: Vec<Value>,
        mid: Vec<Value>,
        outputs: Vec<Value>,
    ) -> Result<(), R1CSError> {
        // Common
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(128, 1);

        // Prover's scope
        let (proof, input_com, mid_com, output_com) = {
            let mut prover_transcript = Transcript::new(b"KMixTest");
            let mut rng = rand::thread_rng();

            let mut prover = Prover::new(&pc_gens, &mut prover_transcript);
            let (input_com, input_vars) = inputs.commit(&mut prover, &mut rng);
            let (mid_com, mid_vars) = mid.commit(&mut prover, &mut rng);
            let (output_com, output_vars) = outputs.commit(&mut prover, &mut rng);

            call_mix_gadget(&mut prover, &input_vars, &mid_vars, &output_vars)?;

            let proof = prover.prove(&bp_gens)?;
            (proof, input_com, mid_com, output_com)
        };

        // Verifier makes a `ConstraintSystem` instance representing a merge gadget
        let mut verifier_transcript = Transcript::new(b"KMixTest");
        let mut verifier = Verifier::new(&mut verifier_transcript);

        let input_vars = input_com.commit(&mut verifier);
        let mid_vars = mid_com.commit(&mut verifier);
        let output_vars = output_com.commit(&mut verifier);

        // Verifier adds constraints to the constraint system
        assert!(call_mix_gadget(&mut verifier, &input_vars, &mid_vars, &output_vars).is_ok());

        Ok(verifier.verify(&proof, &pc_gens, &bp_gens)?)
    }

    // Note: the output vectors for order_by_flavor does not have to be in a particular order,
    // they just has to be grouped by flavor. Thus, it is possible to make a valid change to
    // order_by_flavor but break the tests.
    #[test]
    fn order_by_flavor_test() {
        let pc_gens = PedersenGens::default();
        let mut transcript = Transcript::new(b"OrderByFlavorTest");
        let mut prover_cs = Prover::new(&pc_gens, &mut transcript);

        // k = 1
        assert_eq!(
            order_by_flavor(&vec![yuan(1)], &mut prover_cs).unwrap().1,
            vec![yuan(1)]
        );
        // k = 2
        assert_eq!(
            order_by_flavor(&vec![yuan(1), yuan(2)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(2)]
        );
        assert_eq!(
            order_by_flavor(&vec![yuan(1), peso(2)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), peso(2)]
        );
        // k = 3
        assert_eq!(
            order_by_flavor(&vec![yuan(1), peso(3), yuan(2)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(2), peso(3)]
        );
        // k = 4
        assert_eq!(
            order_by_flavor(&vec![yuan(1), peso(3), yuan(2), peso(4)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(2), peso(3), peso(4)]
        );
        assert_eq!(
            order_by_flavor(&vec![yuan(1), peso(3), peso(4), yuan(2)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(2), peso(4), peso(3)]
        );
        assert_eq!(
            order_by_flavor(&vec![yuan(1), peso(3), zero(), yuan(2)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(2), zero(), peso(3)]
        );
        assert_eq!(
            order_by_flavor(&vec![yuan(1), yuan(2), yuan(3), yuan(4)], &mut prover_cs)
                .unwrap()
                .1,
            vec![yuan(1), yuan(4), yuan(3), yuan(2)]
        );
        // k = 5
        assert_eq!(
            order_by_flavor(
                &vec![yuan(1), yuan(2), yuan(3), yuan(4), yuan(5)],
                &mut prover_cs
            )
            .unwrap()
            .1,
            vec![yuan(1), yuan(5), yuan(4), yuan(3), yuan(2)]
        );
        assert_eq!(
            order_by_flavor(
                &vec![yuan(1), peso(2), yuan(3), peso(4), yuan(5)],
                &mut prover_cs
            )
            .unwrap()
            .1,
            vec![yuan(1), yuan(5), yuan(3), peso(4), peso(2)]
        );
        assert_eq!(
            order_by_flavor(
                &vec![yuan(1), peso(2), zero(), peso(4), yuan(5)],
                &mut prover_cs
            )
            .unwrap()
            .1,
            vec![yuan(1), yuan(5), zero(), peso(4), peso(2)]
        );
    }

    #[test]
    fn combine_by_flavor_test() {
        // k = 2
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), peso(4)]),
            (vec![], vec![yuan(1), peso(4)])
        );
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), yuan(3)]),
            (vec![], vec![zero(), yuan(4)])
        );
        // k = 3
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), peso(4), zero()]),
            (vec![peso(4)], vec![yuan(1), peso(4), zero()])
        );
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), yuan(3), peso(2)]),
            (vec![yuan(4)], vec![zero(), yuan(4), peso(2)])
        );
        assert_eq!(
            combine_by_flavor_helper(&vec![peso(2), yuan(1), yuan(3)]),
            (vec![yuan(1)], vec![peso(2), zero(), yuan(4)])
        );
        // k = 4
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), yuan(1), peso(4), peso(4)]),
            (
                vec![yuan(2), peso(4)],
                vec![zero(), yuan(2), zero(), peso(8)]
            )
        );
        assert_eq!(
            combine_by_flavor_helper(&vec![yuan(1), yuan(2), yuan(3), yuan(4)]),
            (
                vec![yuan(3), yuan(6)],
                vec![zero(), zero(), zero(), yuan(10)]
            )
        );
    }

    fn combine_by_flavor_helper(inputs: &Vec<Value>) -> (Vec<Value>, Vec<Value>) {
        let pc_gens = PedersenGens::default();
        let mut transcript = Transcript::new(b"CombineByFlavorTest");
        let mut prover_cs = Prover::new(&pc_gens, &mut transcript);

        let (allocated_mid, allocated_output) = combine_by_flavor(&inputs, &mut prover_cs).unwrap();
        let mid = allocated_mid
            .iter()
            .map(|allocated| allocated.assignment)
            .collect::<Option<Vec<Value>>>()
            .expect("should be a value");
        let output = allocated_output
            .iter()
            .map(|allocated| allocated.assignment)
            .collect::<Option<Vec<Value>>>()
            .expect("should be a value");

        (mid, output)
    }
}
