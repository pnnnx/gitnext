package gitnextlearn;

import java.net.*;
import java.nio.file.*;
import java.util.*;

/** args: classes dir, out dir, classpath of the rest. Every class of the project is initialized on its own, in
 *  a class loader of its own (so all it needs is initialized again): `init-<n>.exec` is what its static blocks
 *  and field initializers ran, `inits.tsv` names the class. */
public final class Inits {
    public static void main(String[] a) throws Exception {
        Path classes = Paths.get(a[0]), out = Paths.get(a[1]);
        List<URL> urls = new ArrayList<>(List.of(classes.toUri().toURL()));
        for (String p : a[2].split(java.io.File.pathSeparator)) if (!p.isEmpty()) urls.add(Paths.get(p).toUri().toURL());
        List<String> names = new ArrayList<>();
        try (var w = Files.walk(classes)) {
            w.filter(p -> p.toString().endsWith(".class") && !p.toString().contains("META-INF"))
             .forEach(p -> { String r = classes.relativize(p).toString(); names.add(r.substring(0, r.length() - 6).replace('/', '.').replace('\\', '.')); });
        }
        Collections.sort(names);
        Dump.take();
        int n = 0;
        try (var idx = new java.io.PrintWriter(Files.newBufferedWriter(out.resolve("inits.tsv")))) {
            for (String c : names) {
                try (URLClassLoader l = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getPlatformClassLoader())) {
                    Class.forName(c, true, l);
                } catch (Throwable e) {
                    // it needs what a test sets up: what ran until it threw still counts
                }
                Files.write(out.resolve("init-" + n + ".exec"), Dump.take());
                idx.println(n++ + "\t" + c);
            }
        }
    }
}
