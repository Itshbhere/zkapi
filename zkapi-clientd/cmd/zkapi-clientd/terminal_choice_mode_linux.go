package main

import "golang.org/x/sys/unix"

func enableTerminalChoices(fd int) (func(), error) {
	state, err := unix.IoctlGetTermios(fd, unix.TCGETS)
	if err != nil {
		return nil, err
	}
	choices := *state
	// Leave ISIG and output processing intact: Ctrl-C still cancels the setup
	// context, and regular newlines still render correctly.
	choices.Lflag &^= unix.ICANON | unix.ECHO | unix.ECHONL
	choices.Cc[unix.VMIN] = 1
	choices.Cc[unix.VTIME] = 0
	if err := unix.IoctlSetTermios(fd, unix.TCSETS, &choices); err != nil {
		return nil, err
	}
	return func() { _ = unix.IoctlSetTermios(fd, unix.TCSETS, state) }, nil
}
