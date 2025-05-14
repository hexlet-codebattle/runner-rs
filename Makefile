docker-build:
	docker build --platform linux/amd64 --file Containerfile --tag codebattle/runner-rs:latest .
docker-push:
	docker push codebattle/runner-rs
