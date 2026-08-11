.PHONY: all setup deploy wait rbac run explore junit json clean

CLUSTER_NAME := heisensim-demo
NAMESPACE := heisensim-demo

# One command to rule them all
all: setup deploy wait rbac run

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

# Generate and apply least-privilege RBAC
rbac:
	@echo "🔒 Generating least-privilege RBAC..."
	heisensim rbac \
		--namespace $(NAMESPACE) \
		--faults crash,latency,partition
	@echo ""
	@echo "Applying RBAC..."
	heisensim rbac \
		--namespace $(NAMESPACE) \
		--faults crash,latency,partition \
	| kubectl apply -f -
	@echo "✅ RBAC applied."

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

# JUnit XML output (pipe to CI)
junit:
	@echo "📋 Generating JUnit XML report..."
	heisensim run \
		--namespace $(NAMESPACE) \
		--seed 42 \
		--duration 2m \
		--warmup 15s \
		--faults crash,latency,partition \
		--config heisensim.toml \
		--output junit | tee heisensim-report.xml
	@echo ""
	@echo "✅ Report saved to heisensim-report.xml"

# JSON output (pipe to CI or dashboards)
json:
	@echo "📊 Generating JSON report..."
	heisensim run \
		--namespace $(NAMESPACE) \
		--seed 42 \
		--duration 2m \
		--warmup 15s \
		--faults crash,latency,partition \
		--config heisensim.toml \
		--output json | tee heisensim-report.json
	@echo ""
	@echo "✅ Report saved to heisensim-report.json"

# Tear down
clean:
	@echo "🧹 Cleaning up..."
	k3d cluster delete $(CLUSTER_NAME)
	rm -f heisensim-report.xml heisensim-report.json
	@echo "✅ Cluster deleted."
