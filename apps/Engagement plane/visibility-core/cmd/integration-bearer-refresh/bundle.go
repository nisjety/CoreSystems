package main

import (
	"context"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"syscall"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/bearerrefresh"
)

type renewingBearer interface {
	Refresh(context.Context) (bearerrefresh.State, error)
	Check() (bearerrefresh.State, error)
	Run(context.Context, func(bearerrefresh.State, error)) error
}

type bundleWorker struct {
	name   string
	worker renewingBearer
}

// One deployment credential, four separate audiences. These paths and scope
// profiles are fixed by the binary, never supplied by an HTTP/browser request.
func studioBundleConfigs(base bearerrefresh.Config, directory string, create bool) ([]bearerrefresh.Config, error) {
	if base.ServiceID == "" {
		return nil, errors.New("Studio bearer bundle requires an explicit service identity")
	}
	if err := prepareBundleDirectory(directory, create); err != nil {
		return nil, err
	}
	profiles := []struct{ name, capability string }{
		{"integration", "integration-readwrite"},
		{"quarry", "quarry-capture"},
		{"model", "model-draft"},
		{"inference", "inference-draft"},
	}
	configs := make([]bearerrefresh.Config, 0, len(profiles))
	for _, profile := range profiles {
		cfg := base
		cfg.Capability, cfg.TokenFile = profile.capability, filepath.Join(directory, profile.name)
		configs = append(configs, cfg)
	}
	return configs, nil
}

func prepareBundleDirectory(directory string, create bool) error {
	invalid := errors.New("STUDIO_BEARER_DIRECTORY must be an owned mode-0700 normalized absolute directory")
	if !filepath.IsAbs(directory) || filepath.Clean(directory) != directory || directory == string(filepath.Separator) {
		return invalid
	}
	info, err := os.Lstat(directory)
	if os.IsNotExist(err) && create {
		// Create only the configured leaf. Do not recursively create or chmod
		// an operator's broader tree, or follow an existing directory symlink.
		if err = os.Mkdir(directory, 0700); err != nil && !os.IsExist(err) {
			return invalid
		}
		info, err = os.Lstat(directory)
	}
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm() != 0700 {
		return invalid
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Geteuid()) {
		return invalid
	}
	return nil
}

func newBundleWorkers(configs []bearerrefresh.Config, client *http.Client, now func() time.Time) ([]bundleWorker, error) {
	workers := make([]bundleWorker, 0, len(configs))
	for _, cfg := range configs {
		refresher, err := bearerrefresh.New(cfg, client, now)
		if err != nil {
			return nil, err
		}
		workers = append(workers, bundleWorker{filepath.Base(cfg.TokenFile), refresher})
	}
	return workers, nil
}

func runStudioBundle(ctx context.Context, workers []bundleWorker, once, check bool, report func(string, bearerrefresh.State, error)) error {
	if len(workers) == 0 || (once && check) {
		return errors.New("invalid Studio bearer bundle operation")
	}
	if once || check {
		for _, member := range workers {
			if err := ctx.Err(); err != nil {
				return err
			}
			var state bearerrefresh.State
			var err error
			if check {
				state, err = member.worker.Check()
			} else {
				state, err = member.worker.Refresh(ctx)
			}
			if report != nil {
				report(member.name, state, err)
			}
			if err != nil {
				return errors.New("Studio bearer bundle profile is not ready: " + member.name)
			}
		}
		return nil
	}
	return renewStudioBundle(ctx, workers, report)
}

func renewStudioBundle(ctx context.Context, workers []bundleWorker, report func(string, bearerrefresh.State, error)) error {
	group, cancel := context.WithCancel(ctx)
	defer cancel()
	completed := make(chan error, len(workers))
	for _, member := range workers {
		go func(member bundleWorker) {
			err := member.worker.Run(group, func(state bearerrefresh.State, err error) {
				if report != nil {
					report(member.name, state, err)
				}
			})
			if err != nil && !errors.Is(err, context.Canceled) {
				err = errors.New("Studio bearer bundle renewal failed: " + member.name)
			}
			completed <- err
		}(member)
	}
	first := <-completed
	cancel()
	for index := 1; index < len(workers); index++ {
		<-completed
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if first == nil || errors.Is(first, context.Canceled) {
		return errors.New("Studio bearer bundle renewal stopped unexpectedly")
	}
	return first
}
