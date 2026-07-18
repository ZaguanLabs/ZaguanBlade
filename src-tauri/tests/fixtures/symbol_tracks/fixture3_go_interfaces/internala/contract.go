package internala

type refresher interface {
	refresh() error
}

type LocalRefresher struct{}

func (LocalRefresher) refresh() error { return nil }
