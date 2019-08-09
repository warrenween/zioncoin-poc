use crate::encoding::Encodable;
use crate::errors::VMError;
use crate::merkle::MerkleItem;
use crate::ops::Instruction;
use crate::predicate::PredicateTree;
use crate::scalar_witness::ScalarWitness;
use crate::types::String;

use core::borrow::Borrow;
use merlin::Transcript;

/// A builder type for assembling a sequence of `Instruction`s with chained method calls.
/// E.g. `let prog = Program::new().push(...).input().push(...).output(1).to_vec()`.
#[derive(Clone, Debug)]
pub struct Program(Vec<Instruction>);

/// Represents a view of a program.
#[derive(Clone, Debug)]
pub enum ProgramItem {
    /// `ProgramItem::Bytecode` represents the verifier's view - a Vector of bytecode-as-is.
    Bytecode(Vec<u8>),
    /// `ProgramItem::Program` represents the prover's view - a Program struct.
    Program(Program),
}

macro_rules! def_op {
    ($func_name:ident, $op:ident) => (
           /// Adds a `$func_name` instruction.
           pub fn $func_name(&mut self) -> &mut Program{
             self.0.push(Instruction::$op);
             self
        }
    );
    ($func_name:ident, $op:ident, $type:ty) => (
           /// Adds a `$func_name` instruction.
           pub fn $func_name(&mut self, arg :$type) -> &mut Program{
             self.0.push(Instruction::$op(arg));
             self
        }
    );
    ($func_name:ident, $op:ident, $type1:ty, $type2:ty) => (
           /// Adds a `$func_name` instruction.
           pub fn $func_name(&mut self, arg1: $type1, arg2: $type2) -> &mut Program{
             self.0.push(Instruction::$op(arg1, arg2));
             self
        }
    );
}

impl Encodable for Program {
    fn encode(&self, buf: &mut Vec<u8>) {
        for i in self.0.iter() {
            i.borrow().encode(buf);
        }
    }
    fn serialized_length(&self) -> usize {
        self.0.iter().map(|p| p.serialized_length()).sum()
    }
}

impl Program {
    def_op!(add, Add);
    def_op!(alloc, Alloc, Option<ScalarWitness>);
    def_op!(and, And);
    def_op!(borrow, Borrow);
    def_op!(call, Call);
    def_op!(cloak, Cloak, usize, usize);
    def_op!(r#const, Const);
    def_op!(contract, Contract, usize);
    def_op!(drop, Drop);
    def_op!(dup, Dup, usize);
    def_op!(eq, Eq);
    def_op!(expr, Expr);
    def_op!(input, Input);
    def_op!(issue, Issue);
    def_op!(log, Log);
    def_op!(maxtime, Maxtime);
    def_op!(mintime, Mintime);
    def_op!(mul, Mul);
    def_op!(neg, Neg);
    def_op!(or, Or);
    def_op!(output, Output, usize);
    def_op!(range, Range);
    def_op!(retire, Retire);
    def_op!(roll, Roll, usize);
    def_op!(sign_tx, Signtx);
    def_op!(signid, Signid);
    def_op!(signtag, Signtag);
    def_op!(unblind, Unblind);
    def_op!(var, Var);
    def_op!(verify, Verify);

    /// Creates an empty `Program`.
    pub fn new() -> Self {
        Program(vec![])
    }

    /// Creates an empty `Program` and passes its &mut to the closure to let it add the instructions.
    /// Returns the resulting program.
    pub fn build<F>(builder: F) -> Self
    where
        F: FnOnce(&mut Self) -> &mut Self,
    {
        let mut program = Self::new();
        builder(&mut program);
        program
    }
    /// Serializes a Program into a byte array.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    // /// Creates a program from parsing the Bytecode data slice of encoded instructions.
    // pub(crate) fn parse(data: &[u8]) -> Result<Self, VMError> {
    //     SliceReader::parse(data, |r| {
    //         let mut program = Self::new();
    //         while r.len() > 0 {
    //             program.0.push(Instruction::parse(r)?);
    //         }
    //         Ok(program)
    //     })
    // }

    /// Converts the program to a plain vector of instructions.
    pub fn to_vec(self) -> Vec<Instruction> {
        self.0
    }

    /// Adds a `push` instruction with an immediate data that can be converted into `String`.
    pub fn push<T: Into<String>>(&mut self, data: T) -> &mut Program {
        self.0.push(Instruction::Push(data.into()));
        self
    }

    /// Adds a `program` instruction with an immediate data that can be converted into `ProgramItem`.
    pub fn program<T: Into<ProgramItem>>(&mut self, prog: T) -> &mut Program {
        self.0.push(Instruction::Program(prog.into()));
        self
    }

    /// Takes predicate tree and index of program in Merkle tree to verify
    /// the program's membership in that Merkle tree and call the program.
    pub fn choose_call(
        &mut self,
        pred_tree: PredicateTree,
        prog_index: usize,
    ) -> Result<&mut Program, VMError> {
        let (call_proof, program) = pred_tree.create_callproof(prog_index)?;
        self.push(String::Opaque(call_proof.to_bytes()))
            .program(program)
            .call();
        Ok(self)
    }
}

impl Encodable for ProgramItem {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            ProgramItem::Program(prog) => prog.encode(buf),
            ProgramItem::Bytecode(bytes) => {
                buf.extend_from_slice(&bytes);
            }
        }
    }
    fn serialized_length(&self) -> usize {
        match self {
            ProgramItem::Program(prog) => prog.serialized_length(),
            ProgramItem::Bytecode(vec) => vec.len(),
        }
    }
}
impl ProgramItem {
    /// Encodes the program item into a bytecode array.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    /// Downcasts a program item into a program.
    pub fn to_program(self) -> Result<Program, VMError> {
        match self {
            ProgramItem::Program(prog) => Ok(prog),
            ProgramItem::Bytecode(_) => return Err(VMError::TypeNotProgram),
        }
    }

    /// Downcasts a program item into a vector of bytes.
    /// Fails if called on a non-opaque `ProgramItem::Program`.
    /// Use `encode` method to serialize both opaque/nonopaque programs.
    pub fn to_bytecode(self) -> Result<Vec<u8>, VMError> {
        match self {
            ProgramItem::Program(_) => return Err(VMError::TypeNotProgram),
            ProgramItem::Bytecode(bytes) => Ok(bytes),
        }
    }
}

impl From<Program> for ProgramItem {
    fn from(x: Program) -> Self {
        ProgramItem::Program(x)
    }
}

impl MerkleItem for ProgramItem {
    fn commit(&self, t: &mut Transcript) {
        match self {
            ProgramItem::Program(prog) => prog.commit(t),
            ProgramItem::Bytecode(bytes) => t.append_message(b"program", &bytes),
        }
    }
}

impl MerkleItem for Program {
    fn commit(&self, t: &mut Transcript) {
        t.append_message(b"program", &self.to_bytes());
    }
}
