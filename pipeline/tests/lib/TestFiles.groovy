import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.zip.GZIPInputStream
import java.util.zip.GZIPOutputStream

/** File helpers shared by the nf-test suites (nf-test reads this from tests/lib, its default libDir). */
class TestFiles {

    /** The rows of a TSV file, plain or gzip, each split into its columns (trailing empty ones kept). */
    static List<List<String>> rows(Object path) {
        return lines(path).collect { line -> line.split('\t', -1) as List }
    }

    /** The lines of a file, plain or gzip (told by its first two bytes). */
    static List<String> lines(Object path) {
        def file = new File(path.toString())
        def head = new byte[2]
        def n = file.withInputStream { it.read(head) }
        def gzip = n == 2 && (head[0] & 0xff) == 0x1f && (head[1] & 0xff) == 0x8b
        def input = gzip ? new GZIPInputStream(file.newInputStream()) : file.newInputStream()
        return input.withReader('UTF-8') { it.readLines() }
    }

    /** Writes lines to a file, gzipped if its name ends in .gz; returns its path. */
    static String write(Object path, List<String> lines) {
        def file = new File(path.toString())
        file.parentFile.mkdirs()
        def text = lines.collect { it + '\n' }.join('')
        if (file.name.endsWith('.gz')) {
            file.withOutputStream { out -> new GZIPOutputStream(out).withWriter('UTF-8') { it << text } }
        }
        else {
            file.text = text
        }
        return file.path
    }

    /**
     * Lays out a UCSC hg38 download directory under dir, as FETCH_UCSC and FETCH_FASTA read it:
     * database/{ncbiRefSeq,knownGene}.txt.gz are the mini tables, and bigZips/hg38.fa.gz is
     * chrM.fa.gz with bigZips/md5sum.txt holding its md5 on the hg38.fa.gz line. Returns dir as
     * a file:// URL, for --ucsc_base.
     */
    static String ucscMirror(Object projectDir, Object dir) {
        def data = new File("${projectDir}/tests/data")
        def copy = { String from, String to ->
            def target = new File("${dir}/${to}")
            target.parentFile.mkdirs()
            Files.copy(new File(data, from).toPath(), target.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
        copy('ucsc_mini_ncbiRefSeq.txt.gz', 'database/ncbiRefSeq.txt.gz')
        copy('ucsc_mini_knownGene.txt.gz', 'database/knownGene.txt.gz')
        copy('chrM.fa.gz', 'bigZips/hg38.fa.gz')
        copy('chrM_md5sum.txt', 'bigZips/md5sum.txt')
        return "file://${dir}".toString()
    }
}
