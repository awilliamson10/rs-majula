"""Parity smoke for the PyO3 numpy binding over BatchEnv.

Run (from majula/rl-env-py, after `maturin develop`):
    MIRROR_SCENARIO="$(cd ../rl-env && pwd)/scenarios/mirror_melee.ron" \
        pytest tests/test_binding.py -v
"""
import os
import numpy as np
import rs_pk_env


def make(num_duels=2):
    path = os.environ["MIRROR_SCENARIO"]
    return rs_pk_env.BatchEnv(path, num_duels, 1000, 32, 1.0)


def test_shapes_and_reset():
    env = make(2)
    assert env.num_agents == 4
    assert env.obs_stride == 22 and env.act_stride == 6
    obs = env.reset()
    assert obs.shape == (4, 22) and obs.dtype == np.float32
    # self-HP column is 99 at spawn
    assert np.allclose(obs[:, 0], 99.0)


def test_step_antisymmetric_reward():
    env = make(2)
    env.reset()
    acts = np.tile(np.array([0, 1, 0, 0, 0, 0], dtype=np.int32), (4, 1))
    saw = False
    for _ in range(20):
        obs, rew, done = env.step(acts)
        assert obs.shape == (4, 22) and rew.shape == (4,) and done.shape == (4,)
        for i in range(2):
            assert abs(rew[2 * i] + rew[2 * i + 1]) < 1e-5
        if np.any(rew != 0):
            saw = True
    assert saw
