.PHONY: build
build:
	git submodule update --init
	cd build && cmake -GNinja .. && ninja
