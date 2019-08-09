//! Core ZkVM stack types: data, variables, values, contracts etc.

use bulletproofs::r1cs;
use curve25519_dalek::scalar::Scalar;
use merlin::Transcript;
use serde::{Deserialize, Serialize};
use spacesuit::SignedInteger;

use crate::constraints::{Commitment, Constraint, Expression, Variable};
use crate::contract::{Contract, PortableItem};
use crate::encoding::Encodable;
use crate::encoding::SliceReader;
use crate::errors::VMError;
use crate::predicate::Predicate;
use crate::program::ProgramItem;
use crate::scalar_witness::ScalarWitness;
use crate::transcript::TranscriptProtocol;

/// An item on a VM stack.
#[derive(Debug)]
pub enum Item {
    /// A data item: a text string, a commitment or a scalar
    String(String),

    /// A program item: a bytecode string for `call`/`delegate` instructions
    Program(ProgramItem),

    /// A contract.
    Contract(Contract),

    /// A value type.
    Value(Value),

    /// A wide value type.
    WideValue(WideValue),

    /// A variable type.
    Variable(Variable),

    /// An expression type.
    Expression(Expression),

    /// A constraint type.
    Constraint(Constraint),
}

/// An item on a VM stack that can be copied and dropped.
#[derive(Clone, Debug)]
pub enum CopyableItem {
    /// A data item: a text string, a commitment or a scalar
    String(String),

    /// A variable type.
    Variable(Variable),
}

/// A data item.
#[derive(Clone, Debug)]
pub enum String {
    /// Opaque data item.
    Opaque(Vec<u8>),

    /// A predicate.
    Predicate(Box<Predicate>),

    /// A Pedersen commitment.
    Commitment(Box<Commitment>),

    /// A scalar witness (scalar or integer).
    Scalar(Box<ScalarWitness>),

    /// An unspent output (utxo).
    Output(Box<Contract>),
}

/// Represents a value of an issued asset in the VM.
/// Note: values do not necessarily have open commitments. Some can be reblinded,
/// others can be passed-through to an output without going through `cloak` and the constraint system.
#[derive(Clone, Debug)]
pub struct Value {
    /// Commitment to value's quantity
    pub qty: Commitment,
    /// Commitment to value's flavor
    pub flv: Commitment,
}

/// Represents a cleartext value of an issued asset in the VM.
/// This is not the same as `spacesuit::Value` since it is guaranteed to be in-range
/// (negative quantity is not representable with this type).
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ClearValue {
    /// Cleartext quantity integer
    pub qty: u64,
    /// Cleartext flavor scalar
    pub flv: Scalar,
}

/// A wide value type (for negative values created by `borrow`).
#[derive(Debug)]
pub struct WideValue {
    pub(crate) r1cs_qty: r1cs::Variable,
    pub(crate) r1cs_flv: r1cs::Variable,
    pub(crate) witness: Option<(SignedInteger, Scalar)>,
}

impl Item {
    /// Downcasts item to `String` type.
    pub fn to_string(self) -> Result<String, VMError> {
        match self {
            Item::String(x) => Ok(x),
            _ => Err(VMError::TypeNotString),
        }
    }

    /// Downcasts item to `ProgramItem` type.
    pub fn to_program(self) -> Result<ProgramItem, VMError> {
        match self {
            Item::Program(x) => Ok(x),
            _ => Err(VMError::TypeNotProgramItem),
        }
    }

    /// Downcasts item to `Contract` type.
    pub fn to_contract(self) -> Result<Contract, VMError> {
        match self {
            Item::Contract(c) => Ok(c),
            _ => Err(VMError::TypeNotContract),
        }
    }

    /// Downcasts item to `Value` type.
    pub fn to_value(self) -> Result<Value, VMError> {
        match self {
            Item::Value(v) => Ok(v),
            _ => Err(VMError::TypeNotValue),
        }
    }

    /// Downcasts item to `WideValue` type (Value is NOT casted to WideValue).
    pub fn to_wide_value(self) -> Result<WideValue, VMError> {
        match self {
            Item::WideValue(w) => Ok(w),
            _ => Err(VMError::TypeNotWideValue),
        }
    }

    /// Downcasts item to `Variable` type.
    pub fn to_variable(self) -> Result<Variable, VMError> {
        match self {
            Item::Variable(v) => Ok(v),
            _ => Err(VMError::TypeNotVariable),
        }
    }

    /// Downcasts item to `Expression` type (Variable is NOT casted to Expression).
    pub fn to_expression(self) -> Result<Expression, VMError> {
        match self {
            Item::Expression(expr) => Ok(expr),
            _ => Err(VMError::TypeNotExpression),
        }
    }

    /// Downcasts item to `Constraint` type.
    pub fn to_constraint(self) -> Result<Constraint, VMError> {
        match self {
            Item::Constraint(c) => Ok(c),
            _ => Err(VMError::TypeNotConstraint),
        }
    }

    /// Downcasts item to a portable type.
    pub fn to_portable(self) -> Result<PortableItem, VMError> {
        match self {
            Item::String(x) => Ok(PortableItem::String(x)),
            Item::Program(x) => Ok(PortableItem::Program(x)),
            Item::Value(x) => Ok(PortableItem::Value(x)),
            _ => Err(VMError::TypeNotPortable),
        }
    }

    /// Downcasts item to a copyable type.
    pub fn to_copyable(self) -> Result<CopyableItem, VMError> {
        match self {
            Item::String(x) => Ok(CopyableItem::String(x)),
            Item::Variable(x) => Ok(CopyableItem::Variable(x)),
            _ => Err(VMError::TypeNotCopyable),
        }
    }

    /// Copies a copyable type when it's given as a reference.
    pub fn dup_copyable(&self) -> Result<CopyableItem, VMError> {
        match self {
            Item::String(x) => Ok(CopyableItem::String(x.clone())),
            Item::Variable(x) => Ok(CopyableItem::Variable(x.clone())),
            _ => Err(VMError::TypeNotCopyable),
        }
    }
}

impl Encodable for String {
    /// Returns the number of bytes needed to serialize the String.
    fn serialized_length(&self) -> usize {
        match self {
            String::Opaque(data) => data.len(),
            String::Predicate(predicate) => predicate.serialized_length(),
            String::Commitment(commitment) => commitment.serialized_length(),
            String::Scalar(scalar) => scalar.serialized_length(),
            String::Output(output) => output.serialized_length(),
        }
    }
    /// Encodes the data item to an opaque bytestring.
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            String::Opaque(x) => buf.extend_from_slice(x),
            String::Predicate(predicate) => predicate.encode(buf),
            String::Commitment(commitment) => commitment.encode(buf),
            String::Scalar(scalar) => scalar.encode(buf),
            String::Output(contract) => contract.encode(buf),
        };
    }
}

impl String {
    /// Converts the String item into a vector of bytes.
    /// Opaque item is converted without extra allocations,
    /// non-opaque item is encoded to a newly allocated buffer.
    pub fn to_bytes(self) -> Vec<u8> {
        match self {
            String::Opaque(d) => d,
            _ => self.encode_to_vec(),
        }
    }

    /// Downcast the data item to a `Predicate` type.
    pub fn to_predicate(self) -> Result<Predicate, VMError> {
        match self {
            String::Opaque(data) => {
                let point = SliceReader::parse(&data, |r| r.read_point())?;
                Ok(Predicate::Opaque(point))
            }
            String::Predicate(p) => Ok(*p),
            _ => Err(VMError::TypeNotPredicate),
        }
    }

    /// Downcast the data item to a `Commitment` type.
    pub fn to_commitment(self) -> Result<Commitment, VMError> {
        match self {
            String::Opaque(data) => {
                let point = SliceReader::parse(&data, |r| r.read_point())?;
                Ok(Commitment::Closed(point))
            }
            String::Commitment(c) => Ok(*c),
            _ => Err(VMError::TypeNotCommitment),
        }
    }

    /// Downcast the data item to an `Contract` type.
    pub fn to_output(self) -> Result<Contract, VMError> {
        match self {
            String::Opaque(data) => SliceReader::parse(&data, |r| Contract::decode(r)),
            String::Output(i) => Ok(*i),
            _ => Err(VMError::TypeNotOutput),
        }
    }

    /// Downcast the data item to an `ScalarWitness` type.
    pub fn to_scalar(self) -> Result<ScalarWitness, VMError> {
        match self {
            String::Opaque(data) => {
                let scalar = SliceReader::parse(&data, |r| r.read_scalar())?;
                Ok(ScalarWitness::Scalar(scalar))
            }
            String::Scalar(scalar_witness) => Ok(*scalar_witness),
            _ => Err(VMError::TypeNotScalar),
        }
    }
}

impl Default for String {
    fn default() -> Self {
        String::Opaque(Vec::new())
    }
}

impl Value {
    /// Computes a flavor as defined by the `issue` instruction from a predicate.
    pub fn issue_flavor(predicate: &Predicate, metadata: String) -> Scalar {
        let mut t = Transcript::new(b"ZkVM.issue");
        t.append_message(b"predicate", predicate.to_point().as_bytes());
        t.append_message(b"metadata", &metadata.to_bytes());
        t.challenge_scalar(b"flavor")
    }

    /// Returns a (qty,flavor) assignment to a value, or None if both fields are unassigned.
    /// Fails if the assigment is inconsistent.
    pub(crate) fn assignment(&self) -> Result<Option<(SignedInteger, Scalar)>, VMError> {
        match (self.qty.assignment(), self.flv.assignment()) {
            (None, None) => Ok(None),
            (Some(ScalarWitness::Integer(q)), Some(ScalarWitness::Scalar(f))) => Ok(Some((q, f))),
            (_, _) => return Err(VMError::InconsistentWitness),
        }
    }
}

// Upcasting all witness data types to String

impl<T> From<T> for String
where
    T: Into<ScalarWitness>,
{
    fn from(x: T) -> Self {
        String::Scalar(Box::new(x.into()))
    }
}

impl From<Predicate> for String {
    fn from(x: Predicate) -> Self {
        String::Predicate(Box::new(x))
    }
}

impl From<Commitment> for String {
    fn from(x: Commitment) -> Self {
        String::Commitment(Box::new(x))
    }
}

impl From<Contract> for String {
    fn from(x: Contract) -> Self {
        String::Output(Box::new(x))
    }
}

// Upcasting all types to Item

impl From<String> for Item {
    fn from(x: String) -> Self {
        Item::String(x)
    }
}

impl From<ProgramItem> for Item {
    fn from(x: ProgramItem) -> Self {
        Item::Program(x)
    }
}

impl From<Value> for Item {
    fn from(x: Value) -> Self {
        Item::Value(x)
    }
}

impl From<WideValue> for Item {
    fn from(x: WideValue) -> Self {
        Item::WideValue(x)
    }
}

impl From<Contract> for Item {
    fn from(x: Contract) -> Self {
        Item::Contract(x)
    }
}

impl From<Variable> for Item {
    fn from(x: Variable) -> Self {
        Item::Variable(x)
    }
}

impl From<Expression> for Item {
    fn from(x: Expression) -> Self {
        Item::Expression(x)
    }
}

impl From<Constraint> for Item {
    fn from(x: Constraint) -> Self {
        Item::Constraint(x)
    }
}

// Upcast a portable item to any item
impl From<PortableItem> for Item {
    fn from(portable: PortableItem) -> Self {
        match portable {
            PortableItem::String(x) => Item::String(x),
            PortableItem::Program(x) => Item::Program(x),
            PortableItem::Value(x) => Item::Value(x),
        }
    }
}

// Upcast a copyable item to any item
impl From<CopyableItem> for Item {
    fn from(copyable: CopyableItem) -> Self {
        match copyable {
            CopyableItem::String(x) => Item::String(x),
            CopyableItem::Variable(x) => Item::Variable(x),
        }
    }
}
