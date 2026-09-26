"""
Core data models for Banshee.
"""
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import List, Optional
import random

class PlaybackState(Enum):
    STOPPED = auto()
    PLAYING = auto()
    PAUSED = auto()
    BUFFERING = auto()

class RepeatMode(Enum):
    OFF = auto()
    ONE = auto()
    ALL = auto()

@dataclass
class Track:
    id: str
    title: str
    artist: str
    album: str = ""
    duration: float = 0.0  # seconds
    thumbnail_url: str = ""
    stream_url: str = ""
    source_name: str = "YouTube Music"

    def duration_str(self) -> str:
        if self.duration <= 0:
            return "--:--"
        mins = int(self.duration) // 60
        secs = int(self.duration) % 60
        return f"{mins:02d}:{secs:02d}"

@dataclass
class Queue:
    tracks: List[Track] = field(default_factory=list)
    current_index: int = -1
    repeat_mode: RepeatMode = RepeatMode.OFF
    shuffle: bool = False
    _original_order: List[Track] = field(default_factory=list)

    def current_track(self) -> Optional[Track]:
        if 0 <= self.current_index < len(self.tracks):
            return self.tracks[self.current_index]
        return None

    def add(self, track: Track, play_next: bool = False) -> None:
        if play_next and 0 <= self.current_index < len(self.tracks):
            self.tracks.insert(self.current_index + 1, track)
        else:
            self.tracks.append(track)

    def remove(self, index: int) -> Optional[Track]:
        if 0 <= index < len(self.tracks):
            removed = self.tracks.pop(index)
            if index < self.current_index:
                self.current_index -= 1
            elif index == self.current_index and self.current_index >= len(self.tracks):
                self.current_index = len(self.tracks) - 1
            return removed
        return None

    def clear(self) -> None:
        self.tracks.clear()
        self.current_index = -1

    def next(self) -> Optional[Track]:
        if not self.tracks:
            return None

        if self.repeat_mode == RepeatMode.ONE:
            return self.current_track()

        if self.current_index + 1 < len(self.tracks):
            self.current_index += 1
            return self.tracks[self.current_index]
        elif self.repeat_mode == RepeatMode.ALL and len(self.tracks) > 0:
            self.current_index = 0
            return self.tracks[self.current_index]
        return None

    def prev(self) -> Optional[Track]:
        if not self.tracks:
            return None

        if self.current_index - 1 >= 0:
            self.current_index -= 1
            return self.tracks[self.current_index]
        elif self.repeat_mode == RepeatMode.ALL and len(self.tracks) > 0:
            self.current_index = len(self.tracks) - 1
            return self.tracks[self.current_index]
        return None

    def set_shuffle(self, enabled: bool) -> None:
        if self.shuffle == enabled:
            return
        self.shuffle = enabled
        curr = self.current_track()
        if enabled:
            self._original_order = list(self.tracks)
            if curr:
                remaining = [t for t in self.tracks if t != curr]
                random.shuffle(remaining)
                self.tracks = [curr] + remaining
                self.current_index = 0
            else:
                random.shuffle(self.tracks)
        else:
            if self._original_order:
                self.tracks = list(self._original_order)
                if curr and curr in self.tracks:
                    self.current_index = self.tracks.index(curr)
                self._original_order.clear()
