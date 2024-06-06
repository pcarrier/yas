package cli

import "testing"

func Test_toolPath(t *testing.T) {
	tests := []struct {
		name    string
		in      string
		url     string
		frag    string
		wantErr bool
	}{
		{"url", "https://example.com", "https://example.com", "", false},
		{"short", "example.com/foo", "https://example.com/foo", "", false},
		{"shorter", "example.com", "https://example.com", "", false},
		{"path only", "path", "https://oh.yas.tools/path", "", false},
		{"path with fragment", "path#frag", "https://oh.yas.tools/path", "frag", false},
		{"complex path", "some/complex.path", "https://oh.yas.tools/some/complex.path", "", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			url, frag, err := toolPath(tt.in)
			if (err != nil) != tt.wantErr {
				t.Errorf("toolPath() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if url != tt.url {
				t.Errorf("toolPath() got %v, expected %v", url, tt.url)
			}
			if frag != tt.frag {
				t.Errorf("toolPath() got %v, expected %v", frag, tt.frag)
			}
		})
	}
}
