# Running the Slidechain demo

To run Slidechain end-to-end,
you must first build and run a `slidechaind` server instance.

From the `slidechain` directory:

```sh
$ go build ./cmd/slidechaind
$ ./slidechaind
```

This will create and run a new `slidechaind` instance,
generating a random keypair for the custodian account and funding the account with Zioncoin testnet funds.
`slidechaind` will log the custodian account ID and hex-encoded initial block ID:
we will need to use these for future commands.

Next,
we will want to peg in funds from the Zioncoin network.

```sh
$ go build ./cmd/peg
$ ./peg -custodian [custodian account ID] -amount 100 -bcid [initial block ID]
```

This will peg-in 100 lumens to slidechain,
generating and funding a Zioncoin account from which to send the funds,
and also generating a TxVM keypair to send the funds to on slidechain.
Both the Zioncoin account ID and the TxVM keypair will be printed to `stderr` by `peg`.

To peg in non-Lumen assets,
you will need to either peg in an asset already issued on the Zioncoin network,
or issue a custom asset yourself.
You can use the `account` command to create and fund new accounts on the Zioncoin testnet to issue assets and to trust issued assets on the Zioncoin testnet.

When the import is processed,
`slidechaind` will log the TxVM asset ID and the anchor:
we will need the anchor value to build and submit future transactions to slidechain.

Now,
you can build and submit any TxVM transactions to manipulate the imported values.
To build and submit TxVM transactions that move value between accounts on slidechain,
you can use the
[`tx build` command](https://github.com/chain/txvm/blob/main/cmd/tx/example.md)
and submit them to the `slidechaind` server's `/submit` endpoint.

To retire funds back out to the Zioncoin network,
we can use the slidechain `export` command.
The `export` command
will peg out the specified funds to the exporter's Zioncoin account,
which shares an ed25519 private key with the exporter's txvm account.

You can create a new Zioncoin account to receive the pegged-out funds using the `account new` command,
or use the automatically-generated account from our `peg` command earlier.

```sh
$ go build ./cmd/export
$ ./export -prv [exporter prv key] -amount 100 -anchor [import anchor]
```

If you want to export only part of the imported funds,
you need to specify the total amount of the import with `-inputamt` in addition to the amount you want to export with `-amount`.
Then,
any leftover funds will be output back to the initial owner.

```sh
$ ./export -prv [exporter prv key] -amount 50 -inputamt 100 -anchor [import anchor]
```

`slidechaind` will print logs that it is retiring the funds and building a peg-out transaction.
Using the logged transaction hash,
we can check that the transaction hit the network and the funds have been pegged out on
[Zioncoin Expert](https://zioncoin.expert/explorer/testnet/network-activity)
or using the
[Zioncoin Laboratory](https://www.zion.info/laboratory/#explorer?network=test).
