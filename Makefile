.PHONY: yas
yas:
	go build -ldflags '-w -s' .

.PHONY: test
test:
	go test ./...
