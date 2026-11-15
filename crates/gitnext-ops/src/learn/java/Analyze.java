package gitnextlearn;

import java.nio.file.*;
import java.util.*;
import org.jacoco.core.analysis.*;
import org.jacoco.core.data.*;
import org.jacoco.core.internal.data.CRC64;
import org.jacoco.core.tools.ExecFileLoader;

/** args: classes dir, exec dir, out file. For every `<n>.exec` and every source file with a line that ran,
 *  one line: n, tab, `pkg/File.java`, tab, the lines that ran separated by commas. */
public final class Analyze {
    public static void main(String[] a) throws Exception {
        Path classes = Paths.get(a[0]), dir = Paths.get(a[1]);
        // a multi-release build holds a class twice (`META-INF/versions/11/…`): the one whose id the data names ran
        Map<String, List<byte[]>> bytes = new HashMap<>();
        try (var w = Files.walk(classes)) {
            for (Path p : (Iterable<Path>) w.filter(p -> p.toString().endsWith(".class"))::iterator) {
                String rel = classes.relativize(p).toString().replace('\\', '/').replaceFirst("^META-INF/versions/\\d+/", "");
                bytes.computeIfAbsent(rel.substring(0, rel.length() - 6), k -> new ArrayList<>()).add(Files.readAllBytes(p));
            }
        }
        List<Path> execs = new ArrayList<>();
        try (var s = Files.list(dir)) {
            s.filter(p -> p.toString().endsWith(".exec")).sorted().forEach(execs::add);
        }
        try (var out = new java.io.PrintWriter(Files.newBufferedWriter(Paths.get(a[2])))) {
            for (Path e : execs) {
                ExecFileLoader l = new ExecFileLoader();
                l.load(e.toFile());
                ExecutionDataStore store = l.getExecutionDataStore();
                CoverageBuilder b = new CoverageBuilder();
                Analyzer an = new Analyzer(store, b);
                for (ExecutionData d : store.getContents()) {
                    if (!d.hasHits()) continue;
                    for (byte[] c : bytes.getOrDefault(d.getName(), List.of())) {
                        if (CRC64.classId(c) == d.getId()) {
                            an.analyzeClass(c, d.getName());
                            break;
                        }
                    }
                }
                Map<String, TreeSet<Integer>> lines = new TreeMap<>();
                for (IClassCoverage cc : b.getClasses()) {
                    if (cc.getSourceFileName() == null) continue;
                    String f = (cc.getPackageName().isEmpty() ? "" : cc.getPackageName() + "/") + cc.getSourceFileName();
                    for (int i = cc.getFirstLine(); i > 0 && i <= cc.getLastLine(); i++) {
                        int st = cc.getLine(i).getStatus();
                        if (st == ICounter.FULLY_COVERED || st == ICounter.PARTLY_COVERED)
                            lines.computeIfAbsent(f, k -> new TreeSet<>()).add(i);
                    }
                }
                String n = e.getFileName().toString().replace(".exec", "");
                for (var en : lines.entrySet()) {
                    StringBuilder sb = new StringBuilder(n).append('\t').append(en.getKey()).append('\t');
                    String sep = "";
                    for (int i : en.getValue()) {
                        sb.append(sep).append(i);
                        sep = ",";
                    }
                    out.println(sb);
                }
            }
        }
    }
}
