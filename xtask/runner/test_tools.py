import hashlib
import importlib.util
import os
from pathlib import Path
import stat
import tempfile
import unittest

HERE = Path(__file__).resolve().parent

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

corpus = load('corpus', HERE / 'fuzz-corpus.py')
proton = load('proton', HERE / 'prepare-proton.py')
scoreboard = load('scoreboard', HERE.parent / 'scoreboard/run.py')
warm = load('warm', HERE / 'warm-proton.py')

def warmed_cache(cache):
    share=cache/'native/share'
    for relative in proton.DIRECTORIES: (share/relative).mkdir(parents=True)
    launcher=share/'caskade/umu/1.4.4/umu/umu-run';launcher.parent.mkdir(parents=True);launcher.write_text('pinned tool')
    (share/'umu/steamrt3').mkdir()
    build=share/'Steam/compatibilitytools.d/UMU-Proton-test';build.mkdir();(build/'proton').write_text('proton')
    return share

def tree(root):
    return sorted((str(p.relative_to(root)),os.readlink(p) if p.is_symlink() else p.read_text() if p.is_file() else None) for p in root.rglob('*'))

class RunnerTools(unittest.TestCase):
    def test_corpus_discards_links_oversized_and_non_hash_names_and_deduplicates(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'; source.mkdir()
            destination = root / 'cache'
            (source / ('1' * 40)).write_bytes(b'payload')
            (source / ('2' * 40)).write_bytes(b'payload')
            (source / ('3' * 40)).symlink_to(source / ('1' * 40))
            (source / 'not-a-hash').write_bytes(b'untrusted name')
            with (source / ('4' * 40)).open('wb') as stream:
                stream.truncate(corpus.MAX_INPUT + 1)
            corpus.merge(source, destination)
            self.assertEqual([p.name for p in destination.iterdir()], [hashlib.sha1(b'payload', usedforsecurity=False).hexdigest()])
            self.assertEqual(next(destination.iterdir()).read_bytes(), b'payload')
            self.assertFalse(any(p.name.startswith('.corpus-') for p in destination.iterdir()))

    def test_project_seed_names_are_rehashed_into_the_retained_namespace(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);source=root/'source';source.mkdir()
            (source/('seed-'+hashlib.sha256(b'seed').hexdigest())).write_bytes(b'seed')
            corpus.merge(source,root/'cache')
            self.assertEqual([p.name for p in (root/'cache').iterdir()],[hashlib.sha1(b'seed',usedforsecurity=False).hexdigest()])

    def test_corpus_aggregate_limit_is_enforced_and_directory_links_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary); source=root/'source';source.mkdir()
            for index in range(3): (source/f'{index:040x}').write_bytes(bytes([index])*8)
            limit=corpus.MAX_TOTAL
            try:
                corpus.MAX_TOTAL=10
                corpus.merge(source,root/'cache')
                self.assertLessEqual(sum(p.stat().st_size for p in (root/'cache').iterdir()),10)
            finally: corpus.MAX_TOTAL=limit
            (root/'link').symlink_to(source,target_is_directory=True)
            with self.assertRaises(ValueError): corpus.merge(root/'link',root/'other')

    def test_cache_copy_requires_warmed_umu_proton_and_never_changes_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);cache=root/'cache'
            with self.assertRaises(ValueError): proton.prepare(cache,root/'job')
            share=warmed_cache(cache);launcher=share/'caskade/umu/1.4.4/umu/umu-run'
            report=proton.prepare(cache,root/'job')
            self.assertNotIn('HOME',report['environment'])
            self.assertEqual(Path(report['environment']['PROTONPATH']).parent,root/'job/Steam/compatibilitytools.d')
            (root/'job/caskade/umu/1.4.4/umu/umu-run').write_text('job update')
            self.assertEqual(launcher.read_text(),'pinned tool')

    def test_copy_replaces_what_a_restored_target_left_behind(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);share=warmed_cache(root/'cache');job=root/'job'
            tool=share/'umu/steamrt3/pressure-vessel-arm64/bin'
            tool.mkdir(parents=True);(tool/'srt-logger-0.bash').symlink_to('../libexec/steam-runtime-tools-0/logger-0.bash')
            (share/'umu/steamrt3/umu').symlink_to('pressure-vessel-arm64')
            # rust-cache deletes a saved target's files but keeps its links.
            stale=job/'umu/steamrt3/pressure-vessel-arm64/bin'
            stale.mkdir(parents=True);(stale/'srt-logger-0.bash').symlink_to('../libexec/steam-runtime-tools-0/logger-0.bash')
            (job/'umu/steamrt3/umu').symlink_to('pressure-vessel-arm64')
            (stale/'gone-from-the-cache').write_text('stale')
            (job/'kept-outside-the-copied-trees').write_text('other')
            job.joinpath('umu/steamrt3').chmod(0o500)
            try:
                proton.prepare(root/'cache',job)
                proton.prepare(root/'cache',job)
            finally:
                if job.joinpath('umu/steamrt3').exists(): job.joinpath('umu/steamrt3').chmod(0o700)
            for relative in proton.DIRECTORIES: self.assertEqual(tree(job/relative),tree(share/relative))
            self.assertEqual((job/'kept-outside-the-copied-trees').read_text(),'other')

    def test_stale_links_are_removed_without_touching_what_they_reach(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);share=warmed_cache(root/'cache');(share/'umu/steamrt3').chmod(0o500)
            outside=root/'outside';outside.mkdir();(outside/'precious').write_text('keep');outside.chmod(0o500)
            before=(tree(root/'cache'),tree(outside))
            try:
                # A link as the destination, as a copied tree, on the way to one,
                # inside one, and a file where a directory belongs.
                link=root/'link';link.symlink_to(outside,target_is_directory=True)
                proton.prepare(root/'cache',link)
                self.assertFalse(link.is_symlink())
                for destination in ('job-tree','job-parent','job-inner','job-file'): (root/destination).mkdir()
                (root/'job-tree/umu').symlink_to(share/'umu',target_is_directory=True)
                (root/'job-parent/caskade').symlink_to(share/'caskade',target_is_directory=True)
                (root/'job-inner/umu').mkdir();(root/'job-inner/umu/into-cache').symlink_to(share,target_is_directory=True)
                (root/'job-file/Steam').write_text('stale')
                for destination in ('job-tree','job-parent','job-inner','job-file'):
                    proton.prepare(root/'cache',root/destination)
                    self.assertEqual(tree(root/destination/'umu'),tree(share/'umu'))
                self.assertEqual((tree(root/'cache'),tree(outside)),before)
                self.assertEqual(stat.S_IMODE((share/'umu/steamrt3').stat().st_mode),0o500)
                self.assertEqual(stat.S_IMODE(outside.stat().st_mode),0o500)
            finally:
                outside.chmod(0o700)
                for path in [p for p in root.rglob('*') if p.is_dir() and not p.is_symlink()]: path.chmod(0o700)

    def test_destination_overlapping_the_cache_is_refused_before_anything_is_removed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);cache=root/'cache';share=warmed_cache(cache);before=tree(cache)
            alias=root/'alias';alias.symlink_to(cache/'native',target_is_directory=True)
            for destination in (share,cache,root,share/'umu',alias/'share'):
                with self.assertRaises(ValueError): proton.prepare(cache,destination)
            self.assertEqual(tree(cache),before)

    def test_warm_cache_pin_matches_product_and_rejects_unverified_bytes(self):
        product=(HERE.parents[1]/'crates/caskade-installer/src/runtime/umu.rs').read_text()
        self.assertIn(warm.SHA256,product)
        self.assertIn(warm.URL,product)
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError): warm.unpack(b'not verified',Path(temporary))
            self.assertEqual(list(Path(temporary).iterdir()),[])

    def test_scoreboard_cannot_turn_needs_user_or_wrong_exit_into_a_pass(self):
        self.assertEqual(scoreboard.verdict({'type':'needs_user'},'Installed',0),'failed')
        self.assertEqual(scoreboard.verdict({'type':'unsupported'},'Unsupported',3),'passed')
        self.assertEqual(scoreboard.verdict({'type':'unsupported'},'Installed',3),'failed')
        self.assertEqual(scoreboard.verdict({'type':'installed'},'Installed',1),'failed')

    def test_native_junit_preserves_failure_and_skip_and_lifecycle_duration(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'junit.xml'
            path.write_text('<testsuites><testsuite><testcase name="ok" time="1.25"/><testcase name="bad"><failure/></testcase><testcase name="skip"><skipped/></testcase></testsuite></testsuites>')
            parsed=scoreboard.native_results(path)
            self.assertEqual(parsed['ok'],{'status':'passed','lifecycle_ms':1250})
            self.assertEqual(parsed['bad']['status'],'failed')
            self.assertEqual(parsed['skip']['status'],'not_run')

if __name__=='__main__': unittest.main()
