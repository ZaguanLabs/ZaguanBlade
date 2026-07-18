package cache

import "context"

type Incomplete struct{}

func (Incomplete) Ping(ctx context.Context) error { return nil }
