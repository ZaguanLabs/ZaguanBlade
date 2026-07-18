package cache

import "context"

type Memory struct{}

func (Memory) Ping(ctx context.Context) error { return nil }

func (Memory) Set(key string, value []byte) error { return nil }
