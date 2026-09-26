import os
import sys

# Add src to python path for testing
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))

from banshee.models import Track, Queue, PlaybackState, RepeatMode

def test_track_duration_str():
    t1 = Track(id="1", title="Title", artist="Artist", duration=125)
    assert t1.duration_str() == "02:05"

    t2 = Track(id="2", title="Title", artist="Artist", duration=0)
    assert t2.duration_str() == "--:--"

def test_queue_navigation():
    q = Queue()
    t1 = Track(id="1", title="Song 1", artist="Artist 1")
    t2 = Track(id="2", title="Song 2", artist="Artist 2")
    t3 = Track(id="3", title="Song 3", artist="Artist 3")

    q.add(t1)
    q.add(t2)
    q.add(t3)

    assert len(q.tracks) == 3
    assert q.current_track() is None

    # Next
    assert q.next() == t1
    assert q.current_index == 0
    assert q.next() == t2
    assert q.current_index == 1
    assert q.next() == t3
    assert q.current_index == 2
    assert q.next() is None  # End of queue, repeat OFF

    # Prev
    assert q.prev() == t2
    assert q.current_index == 1
    assert q.prev() == t1
    assert q.current_index == 0
    assert q.prev() is None

def test_queue_repeat_modes():
    q = Queue()
    t1 = Track(id="1", title="Song 1", artist="Artist 1")
    t2 = Track(id="2", title="Song 2", artist="Artist 2")
    q.add(t1)
    q.add(t2)
    q.next()  # on t1

    q.repeat_mode = RepeatMode.ONE
    assert q.next() == t1

    q.repeat_mode = RepeatMode.ALL
    q.next()  # on t2
    assert q.next() == t1  # Wraps around to start

def test_queue_add_play_next():
    q = Queue()
    t1 = Track(id="1", title="Song 1", artist="Artist 1")
    t2 = Track(id="2", title="Song 2", artist="Artist 2")
    t_next = Track(id="next", title="Song Next", artist="Artist")
    q.add(t1)
    q.add(t2)
    q.next()  # on t1

    q.add(t_next, play_next=True)
    assert q.tracks[1] == t_next
    assert q.tracks[2] == t2

def test_queue_remove_and_clear():
    q = Queue()
    t1 = Track(id="1", title="Song 1", artist="Artist 1")
    t2 = Track(id="2", title="Song 2", artist="Artist 2")
    q.add(t1)
    q.add(t2)
    q.next()  # on t1 (index 0)

    removed = q.remove(1)
    assert removed == t2
    assert len(q.tracks) == 1
    assert q.current_track() == t1

    q.clear()
    assert len(q.tracks) == 0
    assert q.current_index == -1

def test_queue_shuffle():
    q = Queue()
    for i in range(10):
        q.add(Track(id=str(i), title=f"Song {i}", artist="Artist"))
    q.next()  # index 0

    curr = q.current_track()
    q.set_shuffle(True)
    assert q.shuffle is True
    assert q.current_track() == curr  # Current track pinned at start of shuffled queue
    assert len(q.tracks) == 10

    q.set_shuffle(False)
    assert q.shuffle is False
    assert q.tracks[0].id == "0"
