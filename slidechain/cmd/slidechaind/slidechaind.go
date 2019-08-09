package main

import (
	"context"
	"database/sql"
	"flag"
	"log"
	"net"
	"net/http"

	"github.com/interzioncoin/slingshot/slidechain"
	_ "github.com/mattn/go-sqlite3"
)

func main() {
	ctx := context.Background()

	var (
		addr          = flag.String("addr", "localhost:2423", "server listen address")
		dbfile        = flag.String("db", "slidechain.db", "path to db")
		url           = flag.String("equator", "https://equator-testnet.zion.info", "equator server url")
		blockInterval = flag.Duration("interval", slidechain.DefaultBlockInterval, "expected interval between txvm blocks")
	)

	flag.Parse()

	db, err := sql.Open("sqlite3", *dbfile)
	if err != nil {
		log.Fatalf("error opening db: %s", err)
	}
	defer db.Close()
	c, err := slidechain.GetCustodian(ctx, db, *url, *blockInterval)
	if err != nil {
		log.Fatal(err)
	}

	go c.BS.ExpireBlocks(ctx)

	listener, err := net.Listen("tcp", *addr)
	if err != nil {
		log.Fatal(err)
	}

	log.Printf("listening on %s, initial block ID %x", listener.Addr(), c.InitBlockHash.Bytes())

	http.Handle("/submit", c.S)
	http.HandleFunc("/get", c.S.Get)
	http.HandleFunc("/account", c.Account)
	http.HandleFunc("/prepegin", c.DoPrePegIn)
	http.Serve(listener, nil)
}
