"""
Abstract base class defining pluggable audio backends.
"""
from abc import ABC, abstractmethod
from typing import List, Optional
from banshee.models import Track

class AudioSource(ABC):
    @property
    @abstractmethod
    def name(self) -> str:
        """Name of the source provider (e.g. 'YouTube Music', 'Spotify')."""
        pass

    @abstractmethod
    def search(self, query: str, limit: int = 10) -> List[Track]:
        """Search for tracks matching query."""
        pass

    @abstractmethod
    def get_stream_url(self, track: Track) -> str:
        """Resolve a direct streaming audio URL for the given track."""
        pass

    @abstractmethod
    def is_authenticated(self) -> bool:
        """Return True if user session is authenticated."""
        pass
