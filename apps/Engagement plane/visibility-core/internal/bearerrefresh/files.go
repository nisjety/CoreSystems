package bearerrefresh

import (
	"errors"
	"io"
	"os"
	"path/filepath"
)

func privateDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm() != 0700 {
		return errors.New("bearer output directory must be a regular mode-0700 directory")
	}
	return nil
}

func readPrivate(path string, limit int64) ([]byte, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return nil, errors.New("credential file must be a normalized absolute path")
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || (info.Mode().Perm() != 0600 && info.Mode().Perm() != 0400) || info.Size() > limit {
		return nil, errors.New("credential file must be a protected regular file within the size limit")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, errors.New("credential file could not be opened")
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return nil, errors.New("credential file changed while opening")
	}
	content, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil || len(content) == 0 || int64(len(content)) > limit {
		return nil, errors.New("credential file content is empty or unreadable")
	}
	return content, nil
}

func atomicPrivateWrite(path string, content []byte) error {
	if err := privateDirectory(filepath.Dir(path)); err != nil {
		return err
	}
	if info, err := os.Lstat(path); err == nil {
		if !info.Mode().IsRegular() || (info.Mode().Perm() != 0600 && info.Mode().Perm() != 0400) {
			return errors.New("bearer output target is not a protected regular file")
		}
	} else if !os.IsNotExist(err) {
		return errors.New("bearer output target could not be inspected")
	}
	file, err := os.CreateTemp(filepath.Dir(path), ".bearer-renew-*")
	if err != nil {
		return errors.New("could not stage bearer file")
	}
	defer os.Remove(file.Name())
	if err = file.Chmod(0600); err == nil {
		_, err = file.Write(content)
	}
	if err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err != nil || closeErr != nil {
		return errors.New("could not persist staged bearer file")
	}
	if err := os.Rename(file.Name(), path); err != nil {
		return errors.New("could not replace bearer file")
	}
	return nil
}
