.PHONY: all setup deploy wait run explore clean

CLUSTER_NAME := heisensim-demo
NAMESPACE := heisensim-demo

# One command to rule them all
all: setup deploy wait run

# Create a k3d cluster
setup:
	@echo "🔧 Creating k3d cluster..."
	k3d cluster create $(CLUSTER_NAME) --agents 2 --wait
	@echo "✅ Cluster ready."

# Deploy the demo app
deploy:
	@echo "📦 Deploying demo app..."
	kubectl apply -f manifests.yaml
	@echo "✅ Manifests applied."

# Wait for pods to be ready
wait:
	@echo "⏳ Waiting for pods..."
	kubectl wait --for=condition=ready pod --all -n $(NAMESPACE) --timeout=120s
	@echo "✅ All pods ready."
	kubectl get pods -n $(NAMESPACE)

# Single chaos test run
run:
	@echo "🧪 Running chaos test (seed 42)..."
	heisensim run \
		--namespace $(NAMESPACE) \
		--seed 42 \
		--duration 2m \
		--warmup 15s \
		--faults crash,latency,partition \
		--config heisensim.toml

# Explore mode - multiple seeds
explore:
	@echo "🔬 Exploring 10 seeds..."
	heisensim explore \
		--namespace $(NAMESPACE) \
		--seeds 10 \
		--duration 30s \
		--warmup 10s \
		--faults crash,latency,partition \
		--config heisensim.toml

# Tear down
clean:
	@echo "🧹 Cleaning up..."
	k3d cluster delete $(CLUSTER_NAME)
	@echo "✅ Cluster deleted."
