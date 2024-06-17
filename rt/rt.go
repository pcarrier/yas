package rt

import (
	"fmt"
	"io"
	"net/url"
	"os"
	"path"
	"strings"

	"github.com/birkelund/boltdbcache"
	"github.com/gregjones/httpcache"
	"go.etcd.io/bbolt"
)

/*
#cgo CFLAGS: -I${SRCDIR} -I${SRCDIR}/../lua-5.4.6/src
#cgo LDFLAGS: -L${SRCDIR}/../lib
#cgo darwin,amd64  LDFLAGS: -lrt-darwin-amd64
#cgo darwin,arm64  LDFLAGS: -lrt-darwin-arm64
#cgo linux,amd64   LDFLAGS: -lrt-linux-amd64
#cgo linux,arm64   LDFLAGS: -lrt-linux-arm64
#cgo windows,amd64 LDFLAGS: -lrt-windows-amd64
#cgo windows,arm64 LDFLAGS: -lrt-windows-arm64
#include "rt.h"
*/
import "C"

type RT struct {
	Base      *url.URL
	Tool      string
	Fragment  string
	Args      []string
	Env       map[string]string
	state     *C.lua_State
	db        *bbolt.DB
	httpCache httpcache.Cache
}

func NewRT(envp []string, argv []string) (*RT, error) {
	var home string
	base := "https://oh.yas.tools"

	env := make(map[string]string)

	for _, kv := range envp {
		parts := strings.SplitN(kv, "=", 2)
		name := parts[0]
		value := parts[1]
		env[name] = value
		switch name {
		case "YAS_BASE":
			base = value
		case "YAS_HOME":
			home = value
		}
	}

	if home == "" {
		if homeDir, err := os.UserHomeDir(); err != nil {
			return nil, fmt.Errorf("failed to determine home directory: %v", err)
		} else {
			home = path.Join(homeDir, ".yas.tools")
		}
	}

	if err := os.MkdirAll(home, 0700); err != nil {
		return nil, fmt.Errorf("failed to create database directory: %v", err)
	}

	baseURL, err := url.Parse(base)
	if err != nil {
		return nil, fmt.Errorf("failed to parse base URL %v: %v", baseURL, err)
	}
	tool, frag, err := deriveToolURI(argv[1], baseURL)
	if err != nil {
		return nil, err
	}

	db, err := bbolt.Open(path.Join(home, "db"), 0600, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to open database: %v", err)
	}

	httpCache, err := boltdbcache.NewWithDB(db)
	if err != nil {
		return nil, fmt.Errorf("failed to open cache: %v", err)
	}

	res := RT{
		Base:      baseURL,
		Tool:      tool,
		Fragment:  frag,
		Args:      argv[2:],
		Env:       env,
		db:        db,
		httpCache: httpCache,
	}

	res.state = C.buildLua()

	return &res, nil
}

func (r *RT) Fetch(base *url.URL, module string) (string, error) {
	u, err := url.Parse(module)
	if err != nil {
		return "", err
	}
	cur := base.ResolveReference(u).String()
	switch u.Scheme {
	case "http", "https":
		transport := httpcache.Transport{Cache: r.httpCache}
		client := transport.Client()
		resp, err := client.Get(cur)
		if err != nil {
			return "", err
		}
		if resp.StatusCode == 200 {
			bytes, err := io.ReadAll(resp.Body)
			if err != nil {
				return "", err
			}
			return string(bytes), nil
		} else {
			return "", fmt.Errorf("failed to fetch %s: %s", cur, resp.Status)
		}
	case "file":
		if u.Host != "" {
			u.Path = u.Host + u.Path
		}
		bytes, err := os.ReadFile(u.Path)
		if err != nil {
			return "", err
		}
		return string(bytes), nil
	default:
		return "", fmt.Errorf("fetching %s not supported", u)
	}
}
