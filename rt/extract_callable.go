package rt

import (
	"net/url"
)

func deriveToolURI(in string, base *url.URL) (string, string, error) {
	u, err := base.Parse(in)
	if err != nil {
		return "", "", err
	}
	frag := u.Fragment
	u.Fragment = ""
	return u.String(), frag, nil
}
