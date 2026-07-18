// Go interfaces: method specs become child Method symbols of the interface,
// embedded interfaces contribute Extends edges, and concrete types carry
// pointer/value receiver methods for the implicit-interface miner.
package cachefixture

import "context"

// Base is embedded by Cache below.
type Base interface {
	Ping(ctx context.Context) error
}

// Cache embeds Base and adds two method specs of its own.
type Cache interface {
	Base
	Get(key string) ([]byte, bool)
	Set(key string, value []byte) error
}

// MemoryCache is a complete implementation using pointer receivers.
type MemoryCache struct {
	entries map[string][]byte
}

func (m *MemoryCache) Ping(ctx context.Context) error { return nil }

func (m *MemoryCache) Get(key string) ([]byte, bool) {
	value, ok := m.entries[key]
	return value, ok
}

func (m *MemoryCache) Set(key string, value []byte) error {
	m.entries[key] = value
	return nil
}

// ReadOnlyCache is deliberately incomplete: it never defines Set.
type ReadOnlyCache struct{}

func (r ReadOnlyCache) Ping(ctx context.Context) error { return nil }

func (r ReadOnlyCache) Get(key string) ([]byte, bool) { return nil, false }

// NullCache satisfies only the embedded Base interface, via a value receiver.
type NullCache struct{}

func (n NullCache) Ping(ctx context.Context) error { return nil }
