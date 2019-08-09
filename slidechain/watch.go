package slidechain

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"log"
	"time"

	"github.com/bobg/sqlutil"
	"github.com/chain/txvm/errors"
	"github.com/chain/txvm/protocol/bc"
	"github.com/chain/txvm/protocol/txvm"
	i10rnet "github.com/interzioncoin/starlight/net"
	"github.com/zioncoin/go/clients/equator"
	"github.com/zioncoin/go/xdr"
)

// Runs as a goroutine until ctx is canceled.
func (c *Custodian) watchPegIns(ctx context.Context) {
	defer log.Println("watchPegIns exiting")
	backoff := i10rnet.Backoff{Base: 100 * time.Millisecond}

	var cur equator.Cursor
	err := c.DB.QueryRow("SELECT cursor FROM custodian").Scan(&cur)
	if err != nil && err != sql.ErrNoRows {
		log.Fatal(err)
	}

	for {
		err := c.hclient.StreamTransactions(ctx, c.AccountID.Address(), &cur, func(tx equator.Transaction) {
			log.Printf("handling Zioncoin tx %s", tx.ID)

			var env xdr.TransactionEnvelope
			err := xdr.SafeUnmarshalBase64(tx.EnvelopeXdr, &env)
			if err != nil {
				log.Fatal("error unmarshaling Zioncoin tx: ", err)
			}

			if env.Tx.Memo.Type != xdr.MemoTypeMemoHash {
				return
			}

			nonceHash := (*env.Tx.Memo.Hash)[:]
			for _, op := range env.Tx.Operations {
				if op.Body.Type != xdr.OperationTypePayment {
					continue
				}
				payment := op.Body.PaymentOp
				if !payment.Destination.Equals(c.AccountID) {
					continue
				}

				// This operation is a payment to the custodian's account - i.e., a peg.
				// We update the db to note that we saw this entry on the Zioncoin network.
				// We also populate the amount and asset_xdr with the values in the Zioncoin tx.
				assetXDR, err := payment.Asset.MarshalBinary()
				if err != nil {
					log.Fatalf("marshaling asset xdr: %s", err)
					return
				}
				resulted, err := c.DB.ExecContext(ctx, `UPDATE pegs SET amount=$1, asset_xdr=$2, zioncoin_tx=1 WHERE nonce_hash=$3 AND zioncoin_tx=0`, payment.Amount, assetXDR, nonceHash)
				if err != nil {
					log.Fatalf("updating zioncoin_tx=1 for hash %x: %s", nonceHash, err)
				}

				// We confirm that only a single row was affected by the update query.
				numAffected, err := resulted.RowsAffected()
				if err != nil {
					log.Fatalf("checking rows affected by update query for hash %x: %s", nonceHash, err)
				}
				if numAffected != 1 {
					log.Fatalf("multiple rows affected by update query for hash %x", nonceHash)
				}

				// We update the cursor to avoid double-processing a transaction.
				_, err = c.DB.ExecContext(ctx, `UPDATE custodian SET cursor=$1 WHERE seed=$2`, tx.PT, c.seed)
				if err != nil {
					log.Fatalf("updating cursor: %s", err)
					return
				}

				// Wake up a goroutine that executes imports for not-yet-imported pegs.
				log.Printf("broadcasting import for tx with nonce hash %x", nonceHash)
				c.imports.Broadcast()
			}
		})
		if err == context.Canceled {
			return
		}
		if err != nil {
			log.Printf("error streaming from equator: %s, retrying...", err)
		}
		ch := make(chan struct{})
		go func() {
			time.Sleep(backoff.Next())
			close(ch)
		}()
		select {
		case <-ctx.Done():
			return
		case <-ch:
		}
	}
}

// Runs as a goroutine.
func (c *Custodian) watchExports(ctx context.Context) {
	defer log.Println("watchExports exiting")

	c.RunPin(ctx, "watchExports", func(ctx context.Context, b *bc.Block) error {
		for _, tx := range b.Transactions {
			// Check if the transaction has either expected length for an export tx.
			// Confirm that its input, log, and output entries are as expected.
			// If so, look for a specially formatted log ("L") entry
			// that specifies the Zioncoin asset code to peg out and the Zioncoin recipient account ID.
			if len(tx.Log) != 5 && len(tx.Log) != 7 {
				continue
			}
			if tx.Log[0][0].(txvm.Bytes)[0] != txvm.InputCode {
				continue
			}
			if tx.Log[1][0].(txvm.Bytes)[0] != txvm.LogCode {
				continue
			}

			outputIndex := len(tx.Log) - 2
			if tx.Log[outputIndex][0].(txvm.Bytes)[0] != txvm.OutputCode {
				continue
			}

			exportSeedLogItem := tx.Log[len(tx.Log)-3]
			if exportSeedLogItem[0].(txvm.Bytes)[0] != txvm.LogCode {
				continue
			}
			if !bytes.Equal(exportSeedLogItem[1].(txvm.Bytes), exportContract1Seed[:]) {
				continue
			}

			var info pegOut
			exportRef := tx.Log[1][2].(txvm.Bytes)
			err := json.Unmarshal(exportRef, &info)
			if err != nil {
				continue
			}
			exportedAssetBytes := txvm.AssetID(importIssuanceSeed[:], info.AssetXDR)

			// Record the export in the db,
			// then wake up a goroutine that executes peg-outs on the main chain.
			const q = `INSERT INTO exports (txid, pegout_json) VALUES ($1, $2)`
			_, err = c.DB.ExecContext(ctx, q, tx.ID.Bytes(), exportRef)
			if err != nil {
				return errors.Wrapf(err, "recording export tx %x", tx.ID.Bytes())
			}

			log.Printf("recorded export: %d of txvm asset %x (Zioncoin %x) for %s in tx %x", info.Amount, exportedAssetBytes, info.AssetXDR, info.Exporter, tx.ID.Bytes())

			c.exports.Broadcast()
		}
		return nil
	})
}

// Runs as a goroutine.
func (c *Custodian) watchPegOuts(ctx context.Context, pegouts <-chan pegOut) {
	defer log.Print("watchPegOuts exiting")

	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			const q = `SELECT txid, pegout_json FROM exports WHERE pegged_out IN ($1, $2)`
			var txids, refs [][]byte
			err := sqlutil.ForQueryRows(ctx, c.DB, q, pegOutOK, pegOutFail, func(txid, ref []byte) {
				txids = append(txids, txid)
				refs = append(refs, ref)
			})
			if err != nil {
				log.Fatalf("querying peg-outs: %s", err)
			}
			for i, txid := range txids {
				var p pegOut
				err = json.Unmarshal(refs[i], &p)
				if err != nil {
					log.Fatalf("unmarshaling reference: %s", err)
				}
				p.TxID = txid
				err = c.doPostPegOut(ctx, p)
				if err != nil {
					log.Fatalf("doing post-peg-out: %s", err)
				}
			}
		case p, ok := <-pegouts:
			if !ok {
				log.Fatalf("peg-outs channel closed")
			}
			err := c.doPostPegOut(ctx, p)
			if err != nil {
				log.Fatalf("doing post-peg-out: %s", err)
			}
		}
	}
}
