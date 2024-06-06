package rt

import (
	"fmt"
	"github.com/birkelund/boltdbcache"
	"github.com/gregjones/httpcache"
	"go.etcd.io/bbolt"
	"go.starlark.net/starlark"
	"go.starlark.net/syntax"
	"io"
	"net/url"
	"os"
	"path"
	"strings"
)

type loadEntry struct {
	globals starlark.StringDict
	err     error
}

type RT struct {
	Tool      string
	Fragment  string
	globals   starlark.StringDict
	loadCache map[string]*loadEntry
	db        *bbolt.DB
	httpCache httpcache.Cache
}

func NewRT(envp []string, argv []string) (*RT, error) {
	var home string
	base := "https://oh.yas.tools"

	env := starlark.NewDict(len(envp))
	for _, kv := range envp {
		parts := strings.SplitN(kv, "=", 2)
		name := parts[0]
		value := parts[1]
		switch name {
		case "YAS_BASE":
			base = value
		case "YAS_HOME":
			home = value
		}
		if err := env.SetKey(starlark.String(name), starlark.String(value)); err != nil {
			return nil, fmt.Errorf("failed to set env var %q: %v", kv, err)
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

	frag := ""
	args := starlark.Tuple{}

	baseURL, err := url.Parse(base)
	if err != nil {
		return nil, fmt.Errorf("failed to parse base URL %v: %v", baseURL, err)
	}
	tool, frag, err := deriveToolURI(argv[1], baseURL)
	if err != nil {
		return nil, err
	}
	for _, arg := range argv[2:] {
		args = append(args, starlark.String(arg))
	}

	globals := starlark.StringDict{
		"args": args,
		"env":  env,
		"tool": starlark.String(tool),
		"frag": starlark.String(frag),
	}

	db, err := bbolt.Open(path.Join(home, "db"), 0600, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to open database: %v", err)
	}

	httpCache, err := boltdbcache.NewWithDB(db)
	if err != nil {
		return nil, fmt.Errorf("failed to open cache: %v", err)
	}

	return &RT{
		Tool:      tool,
		Fragment:  frag,
		globals:   globals,
		loadCache: make(map[string]*loadEntry),
		db:        db,
		httpCache: httpCache,
	}, nil
}

func (r *RT) Globals() starlark.StringDict {
	return r.globals
}

func (r *RT) Load(thread *starlark.Thread, module string) (starlark.StringDict, error) {
	e, ok := r.loadCache[module]
	if e == nil {
		if ok {
			// request for package whose loading is in progress
			return nil, fmt.Errorf("cycle in load graph")
		}
		r.loadCache[module] = nil
		var globals starlark.StringDict
		data, err := r.Fetch(thread, module)
		if err == nil {
			thread := r.Thread(module)
			globals, err = starlark.ExecFileOptions(&syntax.FileOptions{}, thread, module, data, r.globals)
		}
		e = &loadEntry{globals, err}
		r.loadCache[module] = e
	}
	return e.globals, e.err
}

func (r *RT) Thread(module string) *starlark.Thread {
	return &starlark.Thread{
		Name: module,
		Load: r.Load,
	}
}

func (r *RT) Fetch(thread *starlark.Thread, module string) (string, error) {
	u, err := url.Parse(module)
	if err != nil {
		return "", err
	}
	base, err := url.Parse(thread.Name)
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
