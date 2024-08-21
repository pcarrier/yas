package rt

import (
	"net/url"
	"strings"
)

func extractCallable(in string, defaultHost string) (string, string, error) {
	u, err := url.Parse(in)
	if u.Opaque != "" {
		u, err = url.Parse("https://" + in)
	}
	if err != nil {
		return "", "", err
	}
	frag := u.Fragment
	u.Fragment = ""
	if u.Host == "" {
		parts := strings.SplitN(u.Path, "/", 2)
		if strings.ContainsRune(parts[0], '.') {
			u.Host = parts[0]
			if len(parts) == 1 {
				u.Path = ""
			} else {
				u.Path = parts[1]
			}
		} else {
			u.Host = defaultHost
		}
	}
	if u.Scheme == "" {
		u.Scheme = "https"
	}
	return u.String(), frag, nil
}
