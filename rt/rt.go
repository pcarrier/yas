package rt

import (
	"fmt"
	"io"
	"mime"
	"net/url"
	"os"
	"path"
	"strings"

	"github.com/birkelund/boltdbcache"
	"github.com/gregjones/httpcache"
	"go.etcd.io/bbolt"
	"go.starlark.net/starlark"
	"go.starlark.net/syntax"
	"golang.org/x/net/html"
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
	defaultHost := "oh.yas.tools"

	env := starlark.NewDict(len(envp))
	for _, kv := range envp {
		parts := strings.SplitN(kv, "=", 2)
		name := parts[0]
		value := parts[1]
		switch name {
		case "YAS_HOST":
			defaultHost = value
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
			home = path.Join(homeDir, ".yas")
		}
	}

	if err := os.MkdirAll(home, 0700); err != nil {
		return nil, fmt.Errorf("failed to create database directory: %v", err)
	}

	tool := "repl"
	frag := ""
	args := starlark.Tuple{}
	if len(argv) > 1 {
		var err error
		tool, frag, err = extractCallable(argv[1], defaultHost)
		if err != nil {
			return nil, err
		}
		for _, arg := range argv[2:] {
			args = append(args, starlark.String(arg))
		}
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

		mu, err := url.Parse(module)
		if err != nil {
			return nil, err
		}
		base, err := url.Parse(thread.Name)
		if err != nil {
			return nil, err
		}
		u := base.ResolveReference(mu)
		data, err := r.Fetch(u)
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

func findFirstYasLink(node *html.Node) string {
	if node.Type == html.ElementNode && node.Data == "link" {
		for _, attr := range node.Attr {
			if attr.Key == "rel" && attr.Val == "yas" {
				for _, attr := range node.Attr {
					if attr.Key == "href" {
						return attr.Val
					}
				}
			}
		}
	}
	for c := node.FirstChild; c != nil; c = c.NextSibling {
		if link := findFirstYasLink(c); link != "" {
			return link
		}
	}
	return ""
}

func (r *RT) Fetch(u *url.URL) (string, error) {
	switch u.Scheme {
	case "http", "https":
		transport := httpcache.Transport{Cache: r.httpCache}
		client := transport.Client()
		resp, err := client.Get(u.String())
		if err != nil {
			return "", err
		}
		if resp.StatusCode == 200 {
			ct := resp.Header.Get("Content-Type")
			mt, _, err := mime.ParseMediaType(ct)
			if err == nil && mt == "text/html" {
				node, err := html.Parse(resp.Body)
				if err != nil {
					return "", fmt.Errorf("parse %q: %v", u, err)
				}
				link := findFirstYasLink(node)
				if link == "" {
					return "", fmt.Errorf("no <link rel=\"yas\" href=\"…\"/> at %v", u)
				}
				lu, err := url.Parse(link)
				if err != nil {
					return "", fmt.Errorf("parse link %q: %v", link, err)
				}
				return r.Fetch(u.ResolveReference(lu))
			}

			bytes, err := io.ReadAll(resp.Body)
			if err != nil {
				return "", fmt.Errorf("read %q: %v", u, err)
			}
			return string(bytes), nil
		} else {
			return "", fmt.Errorf("fetch %q: %s", u, resp.Status)
		}
	case "file":
		if u.Host != "" {
			u.Path = u.Host + u.Path
		}
		info, err := os.Stat(u.Path)
		if err != nil {
			return "", err
		}
		if info.IsDir() {
			u.Path = path.Join(u.Path, "main.star")
		}
		bytes, err := os.ReadFile(u.Path)
		if err != nil {
			return "", fmt.Errorf("failed to read %q: %v", u.Path, err)
		}
		return string(bytes), nil
	default:
		return "", fmt.Errorf("fetching %q not supported", u)
	}
}
