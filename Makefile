debug:
	cargo build

release:
	cargo build --release

install: release
	sudo install sentinel.service /lib/systemd/system/sentinel.service
	sudo install ./target/release/amdgpu-sentinel /usr/local/bin/sentinel
	sudo systemctl enable --now sentinel

uninstall: release
	sudo systemctl disable --now sentinel
	sudo rm /lib/systemd/system/sentinel.service
	sudo rm /usr/local/bin/sentinel
