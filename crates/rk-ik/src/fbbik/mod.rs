// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! The full body biped solver CharaStudio poses characters with: a port of
//! RootMotion FinalIK's `FullBodyBipedIK` solver as the game ships it,
//! file by file and name by name (in snake case) so the two read side by
//! side.
//!
//! - `point`: `IKSolver.Point` and `IKSolver.Node`.
//! - `chain`: `FBIKChain` and its `ChildConstraint`.
//! - `constraint_bend`: `IKConstraintBend`.
//! - `effector`: `IKEffector`.
//! - `mapping`: `IKMapping.BoneMap`, `IKMappingSpine`, `IKMappingBone`,
//!   `IKMappingLimb`.
//! - `solver`: `IKSolver`, `IKSolverFullBody`, `IKSolverFullBodyBiped`.
//! - `tools`: the parts of `V3Tools` and `QuaTools` the solver calls.
//!
//! Transforms are indexes into a [`crate::hierarchy::Hierarchy`]. The
//! transforms outside the body (effector targets, bend goals, the
//! component's root) are given as plain world values. Fields that are
//! private in FinalIK are public here and marked; they hold what the
//! solver works out by itself.

pub mod chain;
pub mod constraint_bend;
pub mod effector;
pub mod mapping;
pub mod point;
pub mod solver;
pub mod tools;

pub use chain::{ChildConstraint, FbikChain, Smoothing};
pub use constraint_bend::IkConstraintBend;
pub use effector::{IkEffector, Place};
pub use mapping::{BoneMap, IkMappingBone, IkMappingLimb, IkMappingSpine};
pub use point::Node;
pub use solver::{BipedReferences, FullBodyBipedChain, FullBodyBipedEffector, IkSolverFullBodyBiped, Invalid};
