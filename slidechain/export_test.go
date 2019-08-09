package slidechain

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"testing"
	"time"

	"github.com/interzioncoin/slingshot/slidechain/mockequator"
	"github.com/interzioncoin/slingshot/slidechain/zioncoin"
	"github.com/zioncoin/go/clients/equator"
	"github.com/zioncoin/go/keypair"
	"github.com/zioncoin/go/xdr"
)

func TestPegOut(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	testdir, err := ioutil.TempDir("", t.Name())
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(testdir)
	db, err := sql.Open("sqlite3", fmt.Sprintf("%s/testdb", testdir))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	hclient := mockequator.New()
	c, err := newCustodian(ctx, db, hclient, DefaultBlockInterval)
	if err != nil {
		t.Fatal(err)
	}

	pegouts := make(chan pegOut)
	go c.pegOutFromExports(ctx, pegouts)

	var lumen xdr.Asset
	lumen.Type = xdr.AssetTypeAssetTypeNative
	lumenXDR, err := lumen.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	var amount int64 = 50
	kp, err := keypair.Random()
	if err != nil {
		t.Fatal(err)
	}
	err = zioncoin.FundAccount(kp.Address())
	if err != nil {
		t.Fatalf("error funding account %s: %s", kp.Address(), err)
	}

	tempAddr, seqnum, err := SubmitPreExportTx(c.hclient, kp, c.AccountID.Address(), lumen, amount)
	if err != nil {
		t.Fatal(err)
	}
	txid := []byte("test")
	var zero32 [32]byte // anchor and pubkey do not matter to test this functionality
	p := pegOut{
		TxID:     txid,
		AssetXDR: lumenXDR,
		TempAddr: tempAddr,
		Seqnum:   int64(seqnum),
		Exporter: kp.Address(),
		Amount:   amount,
		Anchor:   zero32[:],
		Pubkey:   zero32[:],
		State:    pegOutNotYet,
	}
	ref, err := json.Marshal(p)
	if err != nil {
		t.Fatal(err)
	}

	_, err = c.DB.Exec("INSERT INTO exports (txid, pegout_json) VALUES ($1, $2)", txid, ref)
	if err != nil && err != context.Canceled {
		t.Fatal(err)
	}

	c.exports.Broadcast()

	ch := make(chan struct{})

	go func() {
		var cursor equator.Cursor
		for {
			err := c.hclient.StreamTransactions(ctx, kp.Address(), &cursor, func(tx equator.Transaction) {
				log.Printf("received tx: %s", tx.EnvelopeXdr)
				var env xdr.TransactionEnvelope
				err := xdr.SafeUnmarshalBase64(tx.EnvelopeXdr, &env)
				if err != nil {
					t.Fatal(err)
				}
				if env.Tx.SourceAccount.Address() != tempAddr {
					log.Println("source accounts don't match, skipping...")
					return
				}
				if len(env.Tx.Operations) != 2 {
					t.Fatalf("too many operations got %d, want 2", len(env.Tx.Operations))
				}
				op := env.Tx.Operations[0]
				if op.Body.Type != xdr.OperationTypeAccountMerge {
					t.Fatalf("wrong operation type: got %s, want %s", op.Body.Type, xdr.OperationTypeAccountMerge)
				}
				if op.Body.Destination.Address() != kp.Address() {
					t.Fatalf("wrong account merge destination: got %s, want %s", op.Body.Destination.Address(), kp.Address())
				}

				op = env.Tx.Operations[1]
				if op.Body.Type != xdr.OperationTypePayment {
					t.Fatalf("wrong operation type: got %s, want %s", op.Body.Type, xdr.OperationTypePayment)
				}
				paymentOp := op.Body.PaymentOp
				if paymentOp.Destination.Address() != kp.Address() {
					t.Fatalf("incorrect payment destination got %s, want %s", paymentOp.Destination.Address(), kp.Address())
				}
				if paymentOp.Amount != 50 {
					t.Fatalf("got incorrect payment amount %d, want %d", paymentOp.Amount, 50)
				}
				if paymentOp.Asset.Type != xdr.AssetTypeAssetTypeNative {
					t.Fatalf("got incorrect payment asset %s, want lumens", paymentOp.Asset.String())
				}
				close(ch)
			})
			if err != nil {
				log.Printf("error streaming from Horizon: %s, retrying in 1s", err)
				time.Sleep(time.Second)
			}
		}
	}()

	select {
	case <-ctx.Done():
		t.Fatal("context timed out: no peg-out tx seen")
	case <-ch:
	}
	// Wait for peg-out to be written.
	// Avoids closing the database while the watch peg-outs goroutine still needs it.
	<-pegouts
}
