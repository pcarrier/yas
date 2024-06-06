package cli

import (
	"net/url"
	"strings"
)

func toolPath(in string) (string, string, error) {
	u, err := url.Parse(in)
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
			u.Host = "oh.yas.tools"
		}
	}
	if u.Scheme == "" {
		u.Scheme = "https"
	}
	return u.String(), frag, nil
}
