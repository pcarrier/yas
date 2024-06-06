package rt

import (
	"net/url"
	"testing"
)

func Test_toolPath(t *testing.T) {
	tests := []struct {
		name    string
		in      string
		url     string
		frag    string
		wantErr bool
	}{
		{"short with port", "example.com:8080", "https://example.com:8080", "", false},

		{"url", "https://example.com", "https://example.com", "", false},
		{"short", "//example.com/foo", "https://example.com/foo", "", false},
		{"shorter", "//example.com", "https://example.com", "", false},
		{"path only", "path", "https://defaultHost/path", "", false},
		{"path with fragment", "path#frag", "https://defaultHost/path", "frag", false},
		{"complex path", "some/complex.path", "https://defaultHost/some/complex.path", "", false},
	}
	base, err := url.Parse("https://defaultHost")
	if err != nil {
		t.Fatalf("Failed to parse base URL: %v", err)
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			url, frag, err := deriveToolURI(tt.in, base)
			if (err != nil) != tt.wantErr {
				t.Errorf("deriveToolURI() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if url != tt.url {
				t.Errorf("deriveToolURI() got %v, expected %v", url, tt.url)
			}
			if frag != tt.frag {
				t.Errorf("deriveToolURI() got %v, expected %v", frag, tt.frag)
			}
		})
	}
}
