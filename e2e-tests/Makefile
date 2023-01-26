REACTOR_TEST_IMAGE ?= drbd-reactor-e2e:latest

.PHONY: e2e_docker_image
e2e_docker_image:
	docker build -t $(REACTOR_TEST_IMAGE) docker/
