# Project Slingshot

_Accelerating trajectory into interzioncoin space._

Slingshot is a new blockchain architecture under active development,
with a strong focus on scalability, privacy and safety.

The Slingshot project consists of the following sub-protocols and components:

### [ZkVM](zkvm)

A blockchain transaction format that is an evolution of [TxVM](https://github.com/chain/txvm)
with **cloaked assets** and **zero-knowledge smart contracts**.

* [README](zkvm/README.md)
* [ZkVM whitepaper](zkvm/docs/zkvm-design.md)
* [ZkVM specification](zkvm/docs/zkvm-spec.md)
* [Blockchain specification](zkvm/docs/zkvm-blockchain.md)
* [ZkVM API guide](zkvm/docs/zkvm-api.md)

### [Spacesuit](spacesuit)

Interzioncoin’s implementation of _Cloak_, a confidential assets protocol
based on the [Bulletproofs](https://doc.dalek.rs/bulletproofs/index.html) zero-knowledge circuit proof system.

* [Spacesuit README](spacesuit/README.md)
* [Cloak specification](spacesuit/spec.md)

### [Musig](musig)

A pure Rust implementation of the [Simple Schnorr Multi-Signatures](https://eprint.iacr.org/2018/068) by Maxwell, Poelstra, Seurin and Wuille.

* [README](musig/README.md)
* [Musig specification](musig/docs/musig-spec.md)

### [Keytree](keytree)

A _key blinding scheme_ for deriving hierarchies of public keys for [Ristretto](https://ristretto.group)-based signatures.

* [Specification](keytree/keytree.md)

### [Token](token)

An API for issuing assets using ZkVM.

### [Slidechain demo](slidechain)

A demonstration of a minimal [Zioncoin](https://zion.info/) sidechain.
Slidechain allows you to _peg_ funds from the Zioncoin testnet, _import_ then to a _sidechain_,
and later _export_ them back to Zioncoin.

* [Slidechain README](slidechain/Readme.md)
* [Pegging specification](slidechain/Pegging.md)
* [Demo documentation](slidechain/Running.md)

![motocrab-with-ristretto-and-gopher-on-storage](https://user-images.githubusercontent.com/698/53667268-5f30c900-3c25-11e9-91a1-503478c2edc1.png)
