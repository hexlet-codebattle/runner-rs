docker-build:
	docker build --file Containerfile --tag codebattle/runner-rs:latest .
docker-push:
	docker push codebattle/runner-rs
